//! Embeds the Aether icons into aether.exe as Win32 resources without needing rc.exe or windres:
//! writes a COFF object holding a .rsrc section and passes it to the linker.
//!
//! Icon group 1 (aether.ico) is the exe icon and the tray icon; group 2 (aether-off.ico) is the tray
//! icon while processing is off. Regenerate both with assets/brand/make-icons.ps1.

use std::path::PathBuf;

const ICONS: [&str; 2] = ["../assets/brand/aether.ico", "../assets/brand/aether-off.ico"];
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;

struct Resource {
    kind: u32,
    id: u32,
    data: Vec<u8>,
}

fn main() {
    for path in ICONS {
        println!("cargo:rerun-if-changed={path}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Machine type and the matching image-relative (ADDR32NB) relocation type.
    let (machine, reloc) = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default().as_str() {
        "x86_64" => (0x8664u16, 0x0003u16),
        "x86" => (0x014c, 0x0007),
        "aarch64" => (0xaa64, 0x0002),
        arch => {
            println!("cargo:warning=no icon resources for target arch {arch}");
            return;
        }
    };
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut resources = Vec::new();
    let mut next_image = 1;
    for (i, path) in ICONS.iter().enumerate() {
        let ico = std::fs::read(dir.join(path)).unwrap_or_else(|e| panic!("reading {path}: {e}"));
        add_icon_group(&mut resources, i as u32 + 1, &mut next_image, &ico);
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("aether-resources.obj");
    std::fs::write(&out, coff(machine, reloc, resources)).unwrap();
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// Splits an .ico file into one RT_ICON per image plus the RT_GROUP_ICON directory that lists them.
fn add_icon_group(res: &mut Vec<Resource>, group_id: u32, next_image: &mut u32, ico: &[u8]) {
    let u32_at = |o: usize| u32::from_le_bytes(ico[o..o + 4].try_into().unwrap());
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut group = ico[..6].to_vec();
    for i in 0..count {
        let entry = 6 + 16 * i;
        let (size, offset) = (u32_at(entry + 8) as usize, u32_at(entry + 12) as usize);
        // GRPICONDIRENTRY: the ICONDIRENTRY fields up to the byte count, then the RT_ICON id.
        group.extend_from_slice(&ico[entry..entry + 12]);
        group.extend_from_slice(&(*next_image as u16).to_le_bytes());
        res.push(Resource { kind: RT_ICON, id: *next_image, data: ico[offset..offset + size].to_vec() });
        *next_image += 1;
    }
    res.push(Resource { kind: RT_GROUP_ICON, id: group_id, data: group });
}

/// A COFF object with a single .rsrc section: type -> id -> language directories, data entries, data.
fn coff(machine: u16, reloc_type: u16, mut res: Vec<Resource>) -> Vec<u8> {
    const SUBDIR: u32 = 0x8000_0000;
    res.sort_by_key(|r| (r.kind, r.id));
    let mut kinds: Vec<u32> = res.iter().map(|r| r.kind).collect();
    kinds.dedup();
    let dir_size = |entries: usize| 16 + 8 * entries;

    let mut off = dir_size(kinds.len());
    let mut kind_dirs = Vec::new();
    for &k in &kinds {
        kind_dirs.push(off);
        off += dir_size(res.iter().filter(|r| r.kind == k).count());
    }
    let lang_dirs = off;
    off += dir_size(1) * res.len();
    let data_entries = off;
    off += 16 * res.len();
    let mut data_offsets = Vec::new();
    for r in &res {
        off = (off + 7) & !7;
        data_offsets.push(off);
        off += r.data.len();
    }
    let size = (off + 3) & !3;

    let mut s = vec![0u8; size];
    let put = |s: &mut [u8], o: usize, v: u32| s[o..o + 4].copy_from_slice(&v.to_le_bytes());
    // IMAGE_RESOURCE_DIRECTORY with only id entries (NumberOfIdEntries is the u16 at offset 14).
    let dir_header = |s: &mut [u8], o: usize, entries: usize| put(s, o + 12, (entries as u32) << 16);

    dir_header(&mut s, 0, kinds.len());
    for (ki, &k) in kinds.iter().enumerate() {
        put(&mut s, 16 + 8 * ki, k);
        put(&mut s, 16 + 8 * ki + 4, SUBDIR | kind_dirs[ki] as u32);
        let members: Vec<usize> = (0..res.len()).filter(|&i| res[i].kind == k).collect();
        dir_header(&mut s, kind_dirs[ki], members.len());
        for (j, &i) in members.iter().enumerate() {
            let e = kind_dirs[ki] + 16 + 8 * j;
            put(&mut s, e, res[i].id);
            put(&mut s, e + 4, SUBDIR | (lang_dirs + dir_size(1) * i) as u32);
        }
    }
    let mut relocs = Vec::new();
    for (i, r) in res.iter().enumerate() {
        let lang = lang_dirs + dir_size(1) * i;
        let entry = data_entries + 16 * i;
        dir_header(&mut s, lang, 1);
        put(&mut s, lang + 16, 0); // LANG_NEUTRAL
        put(&mut s, lang + 20, entry as u32);
        // IMAGE_RESOURCE_DATA_ENTRY: the data RVA is completed by the linker through a relocation.
        put(&mut s, entry, data_offsets[i] as u32);
        put(&mut s, entry + 4, r.data.len() as u32);
        relocs.push(entry as u32);
        s[data_offsets[i]..data_offsets[i] + r.data.len()].copy_from_slice(&r.data);
    }

    let reloc_ptr = 20 + 40 + size;
    let symbols_ptr = reloc_ptr + 10 * relocs.len();
    let mut o = Vec::with_capacity(symbols_ptr + 22);
    // File header
    o.extend(machine.to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend((symbols_ptr as u32).to_le_bytes());
    o.extend(1u32.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    // Section header: initialized, readable data, 4-byte aligned.
    o.extend(b".rsrc\0\0\0");
    o.extend(0u32.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend((size as u32).to_le_bytes());
    o.extend(60u32.to_le_bytes());
    o.extend((reloc_ptr as u32).to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend((relocs.len() as u16).to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend(0x4030_0040u32.to_le_bytes());
    o.extend(s);
    for r in relocs {
        o.extend(r.to_le_bytes());
        o.extend(0u32.to_le_bytes()); // symbol 0: the section itself
        o.extend(reloc_type.to_le_bytes());
    }
    // Symbol table: the .rsrc section symbol (static), then an empty string table.
    o.extend(b".rsrc\0\0\0");
    o.extend(0u32.to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend([3u8, 0]);
    o.extend(4u32.to_le_bytes());
    o
}
