use std::fs;
use std::io::Write;
use std::path::Path;

/// Fixed run-folder name injected into ProcessOptions — deterministic tests.
pub const RUN: &str = "Plot_2026-08-23_13.46.34";

/// Creates a zip file at `path` with the given (entry_name, bytes) pairs.
pub fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for (name, bytes) in entries {
        if name.ends_with('/') {
            zip.add_directory(name.trim_end_matches('/'), options)
                .unwrap();
        } else {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
    }
    zip.finish().unwrap();
}

/// Minimal valid pack.mcmeta content.
pub const MCMETA: &[u8] = br#"{"pack":{"pack_format":1,"description":"test"}}"#;

/// Standard three-core entry set for a normal pack.
#[allow(dead_code)]
pub fn core_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("pack.mcmeta", MCMETA),
        ("pack.png", b"\x89PNG fake"),
        ("assets/minecraft/textures/blocks/stone.png", b"png"),
    ]
}

/// Creates a zip then flips the encryption bit in every local and central header,
/// mimicking a password-protected archive.
#[allow(dead_code)]
pub fn make_encrypted_zip(path: &Path, entries: &[(&str, &[u8])]) {
    make_zip(path, entries);
    let mut bytes = fs::read(path).unwrap();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if &bytes[i..i + 4] == b"PK\x03\x04" {
            bytes[i + 6] |= 1;
        } else if &bytes[i..i + 4] == b"PK\x01\x02" {
            bytes[i + 8] |= 1;
        }
        i += 1;
    }
    fs::write(path, bytes).unwrap();
}

/// Hand-assembles a stored (uncompressed) zip whose entry names are RAW bytes
/// with the UTF-8 flag unset — how Chinese archivers write GBK names.
#[allow(dead_code)]
pub fn make_raw_zip(path: &Path, entries: &[(Vec<u8>, &[u8])]) {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();
    for (name, data) in entries {
        let crc = crc32fast::hash(data);
        offsets.push(out.len() as u32);
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags: no UTF-8 bit
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&0u32.to_le_bytes()); // dos time+date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name);
        out.extend_from_slice(data);
    }
    for (i, (name, data)) in entries.iter().enumerate() {
        let crc = crc32fast::hash(data);
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&0u16.to_le_bytes()); // method
        central.extend_from_slice(&0u32.to_le_bytes()); // dos time+date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offsets[i].to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    fs::write(path, out).unwrap();
}
