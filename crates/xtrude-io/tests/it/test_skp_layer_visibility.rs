// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A layer switched off in the `.skp` arrives switched off.

use std::collections::HashMap;

use openskp::{Layer, SkpModel};
use xtrude_io::skp::adapter::file_layer_records;

fn layer(name: &str, hidden: bool) -> Layer {
    Layer {
        hidden,
        ..Layer::new(name)
    }
}

fn layers_of(layers: Vec<Layer>) -> HashMap<String, bool> {
    let model = SkpModel {
        layers,
        version: "{20.0.000}".into(),
        ..Default::default()
    };
    file_layer_records(&model)
        .into_iter()
        .map(|r| (r.name, r.visible))
        .collect()
}

fn map(pairs: &[(&str, bool)]) -> HashMap<String, bool> {
    pairs.iter().map(|(n, v)| (n.to_string(), *v)).collect()
}

#[test]
fn test_a_hidden_layer_arrives_hidden() {
    let got = layers_of(vec![
        layer("Cameras", false),
        layer("Camera_FOV_Lines", true),
        layer("Camera_FOV_Volume", true),
    ]);
    assert_eq!(
        got,
        map(&[
            ("Cameras", true),
            ("Camera_FOV_Lines", false),
            ("Camera_FOV_Volume", false)
        ])
    );
}

#[test]
fn test_a_plain_layer_is_still_visible() {
    let got = layers_of(vec![layer("Shells", false), layer("Lids", false)]);
    assert_eq!(got, map(&[("Shells", true), ("Lids", true)]));
}

/// `Layer0` IS Xtrude CAD's default layer; importing it would make a
/// duplicate.
#[test]
fn test_the_default_layer_still_does_not_travel() {
    let got = layers_of(vec![
        layer("Layer0", false),
        layer("Untagged", false),
        layer("Shells", false),
    ]);
    assert_eq!(got, map(&[("Shells", true)]));
}

/// A library that spelt the switch `visible` is still understood. The
/// typed model has one spelling, `hidden`: a layer the library reports as
/// not visible arrives not visible.
#[test]
fn test_a_library_that_said_visible_would_still_be_understood() {
    let old = |name: &str, visible: bool| layer(name, !visible);
    let got = layers_of(vec![old("Shells", false), old("Lids", true)]);
    assert_eq!(got, map(&[("Shells", false), ("Lids", true)]));
}
