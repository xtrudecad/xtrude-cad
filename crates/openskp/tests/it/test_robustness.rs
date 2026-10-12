// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A malformed `.skp` gives an error, never a panic: every fixture,
//! truncated, with bytes flipped and with spans zeroed, through the whole
//! parse. Deterministic (a fixed xorshift sequence), no upstream
//! counterpart.

use crate::common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use common::fixture;

const FIXTURES: [&str; 7] = [
    "SU_File.skp",
    "Untitled.skp",
    "blank_v17.skp",
    "single_material_v17.skp",
    "capilla_quiroz_v17.skp",
    "legacy_v7_synthetic.skp",
    "legacy_v2014_synthetic.skp",
];

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Parse `data` (classic or VFF), the conversion to the model included.
fn parse(data: Vec<u8>) {
    if let Ok(parsed) = openskp::full_parse_bytes(data) {
        let _ = openskp::SkpModel::from_parsed(parsed);
    }
}

fn must_not_panic(name: &str, what: &str, data: Vec<u8>) {
    let r = catch_unwind(AssertUnwindSafe(|| parse(data)));
    assert!(r.is_ok(), "{name}: {what} panicked");
}

/// The classic reader is pure Rust over the file bytes, so a mutated
/// classic file exercises the walker; a mutated VFF file mostly breaks
/// the ZIP CRC, so its `model.dat` is mutated inside a rebuilt ZIP too.
#[test]
fn mutated_fixtures_fail_without_panicking() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let n = original.len();
        for k in 0..24 {
            let cut = n * k / 24;
            must_not_panic(
                name,
                &format!("truncated at {cut}"),
                original[..cut].to_vec(),
            );
        }
        for i in 0..40 {
            let mut data = original.clone();
            for _ in 0..(1 + i % 8) {
                let at = rng.below(n);
                data[at] ^= (rng.next() as u8) | 1;
            }
            must_not_panic(name, &format!("flip #{i}"), data);
        }
        for i in 0..20 {
            let mut data = original.clone();
            let at = rng.below(n);
            let len = 1 + rng.below(64);
            for b in data.iter_mut().skip(at).take(len) {
                *b = 0;
            }
            must_not_panic(name, &format!("zeroed #{i}"), data);
        }
    }
}

/// The VFF walk on a mutated `model.dat`, inside a valid container.
#[test]
fn mutated_vff_model_data_fails_without_panicking() {
    use std::io::{Read, Write};
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    for name in ["Untitled.skp", "SU_File.skp"] {
        let original = std::fs::read(fixture(name)).unwrap();
        let start = original
            .windows(4)
            .position(|w| w == b"PK\x03\x04")
            .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&original[start..])).unwrap();
        let mut model = Vec::new();
        zip.by_name("model.dat")
            .unwrap()
            .read_to_end(&mut model)
            .unwrap();
        for i in 0..60 {
            let mut m = model.clone();
            if i % 3 == 0 {
                m.truncate(rng.below(m.len()));
            } else {
                for _ in 0..(1 + i % 16) {
                    let at = rng.below(m.len());
                    m[at] = rng.next() as u8;
                }
            }
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut zw = zip::ZipWriter::new(&mut buf);
                zw.start_file("model.dat", zip::write::SimpleFileOptions::default())
                    .unwrap();
                zw.write_all(&m).unwrap();
                zw.finish().unwrap();
            }
            let mut data = original[..start].to_vec();
            data.extend(buf.into_inner());
            must_not_panic(name, &format!("model.dat mutation #{i}"), data);
        }
    }
}
