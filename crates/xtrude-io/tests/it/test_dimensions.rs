// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Dimensions through the document.

use crate::common;
use common::*;
use serde_json::json;
use xtrude_core::Axis;
use xtrude_io::{document, igz};
use xtrude_model::commands::AddDimensionCommand;
use xtrude_model::dimension::Dimension;
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

fn roundtrip(s: &Scene, name: &str) -> Scene {
    let tmp = TmpDir::new();
    let path = tmp.join(name);
    document::save_scene(s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    back
}

#[test]
fn test_dimension_survives_document_round_trip() {
    let mut s = Scene::new();
    let mut h = History::new();
    h.execute(
        &mut s,
        &mut AddDimensionCommand::new(Dimension::new(
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(0., 0., 2.),
        )),
    );
    let loaded = roundtrip(&s, "dim.xtrude");
    assert_eq!(loaded.doc.dimensions.len(), 1);
    let d = &loaded.doc.dimensions[0];
    let view = loaded.doc_view();
    assert_eq!(d.a(&view), v3(0., 0., 0.));
    assert_eq!(d.b(&view), v3(4., 0., 0.));
    assert_eq!(d.offset, v3(0., 0., 2.));
    assert_eq!(d.value(&view), 4.0);
}

#[test]
fn test_dimension_style_survives_document_round_trip() {
    let mut s = Scene::new();
    s.dimension_style.decimals = 0;
    s.dimension_style.units = "cm".into();
    s.dimension_style.font_size = 14;
    let loaded = roundtrip(&s, "style.xtrude");
    assert_eq!(loaded.dimension_style.decimals, 0);
    assert_eq!(loaded.dimension_style.units, "cm");
    assert_eq!(loaded.dimension_style.font_size, 14);
}

#[test]
fn test_linear_dimension_survives_document_round_trip() {
    let mut s = Scene::new();
    s.doc.dimensions.push(
        Dimension::new(v3(0., 0., 0.), v3(4., 3., 0.), v3(0., 5., 0.)).with_axis(Some(Axis::X)),
    );
    let back = roundtrip(&s, "linear.xtrude");
    assert_eq!(back.doc.dimensions.len(), 1);
    let d = &back.doc.dimensions[0];
    assert_eq!(d.axis, Some(Axis::X));
    assert!((d.value(&back.doc_view()) - 4.0).abs() < 1e-6);
}

#[test]
fn test_an_unknown_dimension_style_key_survives_the_round_trip() {
    // A style from another version: one key this one does not know.
    let payload = json!({"igz_format": 1, "scene": {
        "vertices": [], "edges": [], "faces": [],
        "dimension_style": {"decimals": 1, "leader_gap": {"mm": 1.5}, "ends": "none"}}});
    let tmp = TmpDir::new();
    let path = tmp.join("other.igz");
    std::fs::write(&path, payload.to_string()).unwrap();
    let mut first = Scene::new();
    igz::load_into(&mut first, &path, None).unwrap();
    let back = roundtrip(&first, "again.xtrude");
    let style = &back.dimension_style;
    assert_eq!(style.extra.get("leader_gap"), Some(&json!({"mm": 1.5})));
    assert_eq!((style.decimals, style.ends.as_str()), (1, "none"));
    assert_eq!(style.units, "mm"); // the default under the file's keys
}
