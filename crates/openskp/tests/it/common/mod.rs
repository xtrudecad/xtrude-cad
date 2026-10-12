// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shared test helpers.

pub mod dump;

use std::path::{Path, PathBuf};

/// A committed upstream fixture (`tests/fixtures/<name>`).
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// `_tlv(tag_hex, payload)`: one record, tag given as its hex spelling.
pub fn tlv(tag_hex: &str, payload: &[u8]) -> Vec<u8> {
    let tag = u16::from_str_radix(tag_hex, 16).expect("hex tag");
    let mut out = tag.to_be_bytes().to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// A minimal `.skp`: the magic, 28 zero bytes, then a ZIP of `entries`
/// (`model.dat` first, empty unless given).
pub fn synthetic_skp(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if !entries.iter().any(|(n, _)| *n == "model.dat") {
            zw.start_file("model.dat", opts).expect("zip");
        }
        for (n, data) in entries {
            zw.start_file(*n, opts).expect("zip");
            zw.write_all(data).expect("zip");
        }
        zw.finish().expect("zip");
    }
    let mut data = vec![0xFF, 0xFE, 0xFF, 0x0E];
    data.extend_from_slice(&[0u8; 28]);
    data.extend_from_slice(buf.get_ref());
    let path = dir.join(name);
    std::fs::write(&path, data).expect("write");
    path
}

/// A fresh scratch directory under the target dir.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openskp-test-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}
