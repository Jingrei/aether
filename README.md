<p align="center"><img src="ui/logo.png" alt="Aether" width="280"></p>

# Aether Sound

System-wide spatial audio and enhancement for Windows: virtual 7.1.4 surround on headphones or speakers,
room simulation, bass, dialog and detail enhancement, a 10-band EQ, headphone correction, custom profiles and a tray icon.

**New here? Follow the [step-by-step setup and user guide](docs/GETTING-STARTED.md).**

See [docs/dts-sound-unbound-analysis.md](docs/dts-sound-unbound-analysis.md) for the analysis of DTS Sound Unbound that the
feature set is modeled on.

## Stack

| Part | Choice | Why |
|---|---|---|
| DSP engine (`engine/`) | Rust, `realfft` | Real-time safe (no GC, no allocation on the audio thread), C++-level speed, unit-testable |
| Audio I/O (`app/src/audio.rs`) | `cpal` / WASAPI | Shared-mode capture, loopback and playback with clock-drift compensation |
| UI (`ui/`) | HTML/CSS/JS embedded in the exe, opened as an Edge app window | No extra runtime; served on `127.0.0.1:47820` |
| Tray (`app/src/tray.rs`) | `tray-icon` | On/off, output mode, profile, quality, start with Windows |

## Build and run

Requires Rust (installed per-user with `rustup`).

```powershell
cargo test --workspace                  # 14 DSP tests
cargo build --release
.\target\release\aether.exe             # tray icon + control panel
.\target\release\aether.exe --background
```

Offline rendering (A/B comparisons, testing):

```powershell
aether render in.wav out.wav --profile movies_spacious --mode headphones --quality best
aether devices
aether profiles
```

## Routing system audio

1. Install a virtual audio cable (VB-Audio Virtual Cable is already installed on this PC).
2. Set **CABLE Input** as the Windows default output.
3. Start Aether. With Source / Play on left on *Automatic*, it captures *CABLE Output* and plays on your real device.
4. For discrete surround from games and movies, set the cable to 7.1 under *Configure speakers*. Each channel is then
   rendered at its own virtual position.

## Signal flow

```
input 2.0…7.1.4 ─► upmix / channel map ─► 7.1.4 bed ─► dialog (C) ─► BRIR convolution ─┬─► XTC (speakers mode)
       test object ─┘                                    └─► FDN reverb ──────┘
  ─► detail ─► bass ─► 10-band EQ ─► headphone tuning ─► leveler ─► gain ─► limiter ─► output
```

Settings are saved in `%APPDATA%\Aether\config.json`.
