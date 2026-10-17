# Regenerates the derived icons and UI logo from the source art in this folder.
#   powershell -ExecutionPolicy Bypass -File assets\brand\make-icons.ps1
#
#   aether.ico      exe icon, tray icon (on), control panel favicon   <- white.png
#   aether-off.ico  tray icon while processing is off (greyed)        <- white.png
#   ui/logo.png     control panel header                              <- horizontal-white.png

Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$ui = Join-Path (Split-Path (Split-Path $here)) 'ui'

# Crops to the opaque pixels; with -Square, centers the result on a square transparent canvas.
function Get-Trimmed([System.Drawing.Bitmap]$src, [switch]$Square) {
    $minX = $src.Width; $minY = $src.Height; $maxX = -1; $maxY = -1
    for ($y = 0; $y -lt $src.Height; $y++) {
        for ($x = 0; $x -lt $src.Width; $x++) {
            if ($src.GetPixel($x, $y).A -gt 0) {
                if ($x -lt $minX) { $minX = $x }; if ($x -gt $maxX) { $maxX = $x }
                if ($y -lt $minY) { $minY = $y }; if ($y -gt $maxY) { $maxY = $y }
            }
        }
    }
    $w = $maxX - $minX + 1; $h = $maxY - $minY + 1
    $cw = $w; $ch = $h
    if ($Square) { $cw = [Math]::Max($w, $h); $ch = $cw }
    $out = New-Object System.Drawing.Bitmap $cw, $ch, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($out)
    $g.DrawImage($src, (New-Object System.Drawing.Rectangle ([int](($cw - $w) / 2)), ([int](($ch - $h) / 2)), $w, $h),
        (New-Object System.Drawing.Rectangle $minX, $minY, $w, $h), [System.Drawing.GraphicsUnit]::Pixel)
    $g.Dispose()
    $out
}

# Maps the brand blue to a dark grey and the white bands to a light grey, keeping antialiasing.
function Get-Greyed([System.Drawing.Bitmap]$src) {
    $out = New-Object System.Drawing.Bitmap $src
    $blue = 0x70; $line = @(128, 134, 142); $fill = @(214, 218, 224)
    for ($y = 0; $y -lt $out.Height; $y++) {
        for ($x = 0; $x -lt $out.Width; $x++) {
            $p = $out.GetPixel($x, $y)
            if ($p.A -eq 0) { continue }
            $t = [Math]::Min(1.0, [Math]::Max(0.0, (255 - $p.R) / (255 - $blue)))
            $c = 0..2 | ForEach-Object { [int]($fill[$_] + ($line[$_] - $fill[$_]) * $t) }
            $out.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($p.A, $c[0], $c[1], $c[2]))
        }
    }
    $out
}

function Get-Resized([System.Drawing.Bitmap]$src, [int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $attr = New-Object System.Drawing.Imaging.ImageAttributes
    $attr.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
    $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $size, $size), 0, 0, $src.Width, $src.Height, [System.Drawing.GraphicsUnit]::Pixel, $attr)
    $g.Dispose()
    $bmp
}

# One ICO image: PNG for 256 px, a 32-bit DIB (BGRA bottom-up + 1-bit AND mask) for smaller sizes.
function Get-IcoImage([System.Drawing.Bitmap]$src, [int]$size) {
    $bmp = Get-Resized $src $size
    $ms = New-Object System.IO.MemoryStream
    if ($size -ge 256) {
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        $data = $bmp.LockBits((New-Object System.Drawing.Rectangle 0, 0, $size, $size), 'ReadOnly', ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb))
        $px = New-Object byte[] ($size * $size * 4)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $px, 0, $px.Length)
        $bmp.UnlockBits($data)
        $maskStride = [int]([Math]::Ceiling($size / 32.0) * 4)
        $w = New-Object System.IO.BinaryWriter $ms
        $w.Write([uint32]40); $w.Write([int32]$size); $w.Write([int32]($size * 2)); $w.Write([uint16]1); $w.Write([uint16]32)
        $w.Write([uint32]0); $w.Write([uint32]($px.Length + $maskStride * $size)); $w.Write([int32]0); $w.Write([int32]0); $w.Write([uint32]0); $w.Write([uint32]0)
        for ($y = $size - 1; $y -ge 0; $y--) { $w.Write($px, $y * $size * 4, $size * 4) }
        for ($y = $size - 1; $y -ge 0; $y--) {
            $row = New-Object byte[] $maskStride
            for ($x = 0; $x -lt $size; $x++) {
                if ($px[($y * $size + $x) * 4 + 3] -eq 0) { $row[$x -shr 3] = $row[$x -shr 3] -bor (0x80 -shr ($x -band 7)) }
            }
            $w.Write($row)
        }
        $w.Flush()
    }
    $bmp.Dispose()
    , $ms.ToArray()
}

function Write-Ico([System.Drawing.Bitmap]$src, [int[]]$sizes, [string]$path) {
    $images = @($sizes | ForEach-Object { , (Get-IcoImage $src $_) })
    $ms = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $ms
    $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $s = $sizes[$i] % 256
        $w.Write([byte]$s); $w.Write([byte]$s); $w.Write([byte]0); $w.Write([byte]0)
        $w.Write([uint16]1); $w.Write([uint16]32)
        $w.Write([uint32]$images[$i].Length); $w.Write([uint32]$offset)
        $offset += $images[$i].Length
    }
    foreach ($img in $images) { $w.Write($img) }
    $w.Flush()
    [System.IO.File]::WriteAllBytes($path, $ms.ToArray())
}

$mark = Get-Trimmed ([System.Drawing.Bitmap]::FromFile((Join-Path $here 'white.png'))) -Square
Write-Ico $mark @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256) (Join-Path $here 'aether.ico')
Write-Ico (Get-Greyed $mark) @(16, 20, 24, 32, 40, 48, 64) (Join-Path $here 'aether-off.ico')

$logo = Get-Trimmed ([System.Drawing.Bitmap]::FromFile((Join-Path $here 'horizontal-white.png')))
$logo.Save((Join-Path $ui 'logo.png'), [System.Drawing.Imaging.ImageFormat]::Png)

Write-Host "Wrote aether.ico, aether-off.ico and ui/logo.png"
