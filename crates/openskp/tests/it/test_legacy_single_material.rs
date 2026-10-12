// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP `packages/python/tests/test_legacy_single_material.py`:
//! classic files with fewer than two materials, whose slot base comes from
//! the definition-list anchor (`_probe_layer_anchor_bases`).

use crate::common;

use common::fixture;
use openskp::legacy::is_legacy;

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

#[test]
fn test_detects_both_fixtures_as_legacy() {
    assert!(is_legacy(&read("blank_v17.skp")));
    assert!(is_legacy(&read("single_material_v17.skp")));
}

#[test]
fn test_parses_a_zero_material_legacy_file() {
    let model = openskp::SkpFile::open(fixture("blank_v17.skp"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(model.version, "{17.0.1}");
    assert!(model.materials.is_empty());
    let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Layer0"]);
    assert!(model.definitions.is_empty());
    assert!(model.root.instances.is_empty());
}

#[test]
fn test_parses_a_single_material_legacy_file() {
    let model = openskp::SkpFile::open(fixture("single_material_v17.skp"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(model.version, "{17.0.1}");
    assert_eq!(model.materials.len(), 1);
    assert_eq!(model.materials[0].name, "RedMat");
    let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Layer0"]);
}
