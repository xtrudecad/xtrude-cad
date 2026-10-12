// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP
//! `packages/python/tests/test_legacy_pre2014_instance_guid.py`: classic
//! instances carry a trailing GUID only from SketchUp 2014 (version 14) on.

use crate::common;

use common::fixture;
use openskp::legacy::is_legacy;
use openskp::{AttrValue, SkpModel};

fn parse(name: &str) -> SkpModel {
    openskp::SkpFile::open(fixture(name))
        .unwrap()
        .parse()
        .unwrap()
}

fn names(model: &SkpModel) -> Vec<&str> {
    model
        .root
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

#[test]
fn test_v7_fixture_is_detected_as_legacy() {
    assert!(is_legacy(
        &std::fs::read(fixture("legacy_v7_synthetic.skp")).unwrap()
    ));
}

#[test]
fn test_v2014_fixture_is_detected_as_legacy() {
    assert!(is_legacy(
        &std::fs::read(fixture("legacy_v2014_synthetic.skp")).unwrap()
    ));
}

#[test]
fn test_v7_parses_without_the_attribute_named_slot_collision() {
    assert_eq!(parse("legacy_v7_synthetic.skp").version, "{7.0.1}");
}

#[test]
fn test_v7_places_every_root_instance() {
    assert_eq!(
        names(&parse("legacy_v7_synthetic.skp")),
        ["", "P-1", "P-2", "P-3", "P-4", "P-5", "P-6", "P-7", "P-8", "P-9"]
    );
}

#[test]
fn test_v7_and_v2014_parse_to_the_same_structure() {
    let v7 = parse("legacy_v7_synthetic.skp");
    let v14 = parse("legacy_v2014_synthetic.skp");
    assert_eq!(v7.definitions.len(), 3);
    assert_eq!(v14.definitions.len(), 3);
    assert_eq!(v7.layers.len(), 2);
    assert_eq!(v14.layers.len(), 2);
    assert_eq!(names(&v7), names(&v14));
    let mut v7_names: Vec<&str> = v7.definitions.values().map(|d| d.name.as_str()).collect();
    v7_names.sort();
    assert_eq!(v7_names, ["Frame", "Panel", "Post"]);
    for d7 in v7.definitions.values() {
        let d14 = v14
            .definitions
            .values()
            .find(|d| d.name == d7.name)
            .unwrap();
        assert_eq!(d7.faces.len(), d14.faces.len());
        let inst = |d: &openskp::Definition| -> Vec<String> {
            d.instances.iter().map(|i| i.name.clone()).collect()
        };
        assert_eq!(inst(d7), inst(d14));
    }
}

#[test]
fn test_v7_nested_group_attribute_dictionary_survives() {
    let model = parse("legacy_v7_synthetic.skp");
    let panel = model
        .definitions
        .values()
        .find(|d| d.name == "Panel")
        .unwrap();
    assert_eq!(panel.instances.len(), 1);
    let inner = &panel.instances[0];
    assert_eq!(inner.name, "InnerFrame");
    assert_eq!(inner.attribute_dictionaries.len(), 1);
    let info = &inner.attribute_dictionaries["test-info"];
    assert_eq!(info.len(), 3);
    assert_eq!(info["role"], AttrValue::Str("brace".into()));
    assert_eq!(info["count"], AttrValue::Int(3));
    assert_eq!(info["length"], AttrValue::Float(12.5));
}

#[test]
fn test_v2014_instance_guids_are_still_read() {
    assert_eq!(parse("legacy_v2014_synthetic.skp").root.instances.len(), 10);
}
