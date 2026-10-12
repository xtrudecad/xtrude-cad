// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `SkpFile::parse` timing on the committed fixtures: the largest file
//! (a classic SketchUp 2014 one), the classic file with the most geometry
//! and the largest VFF (2021+) file. UPSTREAM.md compares the numbers with
//! Python OpenSKP.

use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn bench_parse(c: &mut Criterion) {
    for name in [
        "legacy_v2014_synthetic.skp",
        "capilla_quiroz_v17.skp",
        "Untitled.skp",
    ] {
        let file = openskp::SkpFile::open(fixture(name)).expect("fixture");
        c.bench_function(&format!("parse {name}"), |b| {
            b.iter(|| file.parse().expect("parse"));
        });
    }
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
