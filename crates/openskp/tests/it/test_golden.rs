// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Golden parses: every upstream fixture parses to the model Python
//! OpenSKP parsed (with the Python code's `CDimensionLinear` fix), field
//! for field and in the same order. The expected dumps are frozen
//! references (UPSTREAM.md, Golden parses).

use crate::common;

use std::path::{Path, PathBuf};

use common::dump::{dump, first_difference};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn check(name: &str) {
    let skp = fixtures().join(format!("{name}.skp"));
    let expected = fixtures().join(format!("{name}.expected.json"));
    let model = openskp::SkpFile::open(&skp)
        .and_then(|f| f.parse())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = std::fs::read_to_string(&expected).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    if let Some(diff) = first_difference("$", &want, &dump(&model), 1e-9) {
        panic!("{name}: {diff}");
    }
}

#[test]
fn golden_untitled() {
    check("Untitled");
}

#[test]
fn golden_su_file() {
    check("SU_File");
}

#[test]
fn golden_blank_v17() {
    check("blank_v17");
}

#[test]
fn golden_single_material_v17() {
    check("single_material_v17");
}

#[test]
fn golden_capilla_quiroz_v17() {
    check("capilla_quiroz_v17");
}

#[test]
fn golden_legacy_v7_synthetic() {
    check("legacy_v7_synthetic");
}

#[test]
fn golden_legacy_v2014_synthetic() {
    check("legacy_v2014_synthetic");
}

#[test]
fn every_fixture_has_a_golden_dump() {
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(fixtures()).expect("fixtures") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "skp") {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            if !fixtures().join(format!("{stem}.expected.json")).exists() {
                missing.push(stem.into_owned());
            }
        }
    }
    assert!(missing.is_empty(), "no dump for {missing:?}");
}

#[test]
fn the_comparison_reports_the_first_difference() {
    let want = serde_json::json!({"a": [1, {"b": 2.0}], "c": "x"});
    let got = serde_json::json!({"a": [1, {"b": 2.5}], "c": "x"});
    assert_eq!(
        first_difference("$", &want, &got, 1e-9).as_deref(),
        Some("$.a[1].b: expected 2, got 2.5")
    );
    assert_eq!(first_difference("$", &want, &want, 1e-9), None);
}
