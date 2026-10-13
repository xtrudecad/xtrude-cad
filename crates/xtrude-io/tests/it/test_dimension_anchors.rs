// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Dimension anchors through the document.

use crate::common;
use common::*;
use serde_json::json;
use xtrude_io::{document, igz};
use xtrude_model::commands::AddDimensionCommand;
use xtrude_model::dimension::Dimension;
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

#[test]
fn test_the_document_round_trip_binds_again() {
    let mut s = Scene::new();
    s.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
        &[],
    );
    let mut h = History::new();
    h.execute(
        &mut s,
        &mut AddDimensionCommand::new(Dimension::new(
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(0., -1., 0.),
        )),
    );
    let tmp = TmpDir::new();
    let path = tmp.join("dimension.xtrude");
    document::save_scene(&s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    assert!(back.doc.dimensions[0].anchored());
    let v = back.mesh().vertex_at(v3(4., 0., 0.)).unwrap();
    back.mesh_mut().move_vertex(v, v3(1., 0., 0.));
    let d = back.doc.dimensions[0].clone();
    assert!((d.value(&back.doc_view()) - 5.0).abs() < 1e-9);
}

#[test]
fn test_new_documents_take_arrows_and_old_ones_keep_their_ticks() {
    assert_eq!(Scene::new().dimension_style.ends, "arrow");
    let mut s = Scene::new();
    s.dimension_style.ends = "none".into();
    let tmp = TmpDir::new();
    let path = tmp.join("ends.xtrude");
    document::save_scene(&s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    assert_eq!(back.dimension_style.ends, "none");
    let payload = json!({"igz_format": 1, "scene": {
        "vertices": [], "edges": [], "faces": [],
        "dimension_style": {"decimals": 3, "units": "cm"}}});
    let old = tmp.join("old.igz");
    std::fs::write(&old, payload.to_string()).unwrap();
    let mut back = Scene::new();
    igz::load_into(&mut back, &old, None).unwrap();
    assert_eq!(back.dimension_style.ends, "tick");
    assert_eq!(back.dimension_style.decimals, 3);
}
