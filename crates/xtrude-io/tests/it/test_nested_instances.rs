// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Nested instances through the document.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::document;
use xtrude_model::group::{add_mesh, iter_placements, world_mesh, Group};
use xtrude_model::scene::Scene;

#[test]
fn test_document_round_trip_keeps_the_tree_and_writes_the_prototype_once() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    quad(&mut m, 0., 1., 0., 1., 0.);
    let proto = add_mesh(&mut s.doc.meshes, m);
    let at = |x: f64, y: f64, z: f64| Some(DMat4::from_translation(v3(x, y, z)));
    let mut leaf_a = Group::new(proto, Some("leaf A"));
    leaf_a.xform = at(0., 0., 0.);
    let mut leaf_b = Group::new(proto, Some("leaf B"));
    leaf_b.xform = at(10., 0., 0.);
    let mut mid = Group::new(proto, Some("mid"));
    mid.xform = at(0., 100., 0.);
    mid.children = vec![leaf_a, leaf_b];
    let mut top = Group::new(proto, Some("top"));
    top.xform = at(0., 0., 1000.);
    top.children = vec![mid];
    top.layer = Some("fasteners".into());
    s.doc.groups.push(top);
    let tmp = TmpDir::new();
    let path = tmp.join("screw.xtrude");
    document::save_scene(&s, &path).unwrap();
    let raw = payload(&path);
    assert_eq!(raw["protos"].as_array().unwrap().len(), 1);
    assert_eq!(raw["protos"][0]["faces"].as_array().unwrap().len(), 1);
    assert_eq!(raw["groups"].as_array().unwrap().len(), 1);
    let kids = raw["groups"][0]["children"].as_array().unwrap();
    assert!(kids.len() == 1 && kids[0]["children"].as_array().unwrap().len() == 2);
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    assert_eq!(back.doc.groups.len(), 1);
    let g = &back.doc.groups[0];
    assert_eq!(g.layer.as_deref(), Some("fasteners"));
    assert_eq!(world_mesh(&back.doc.meshes, g).faces.len(), 4);
    // One prototype placed at three levels with three different contents
    // is three definitions once open; the two leaves still share.
    let placed = iter_placements(g, None);
    let meshes: std::collections::HashSet<_> = placed.iter().map(|(pg, _)| pg.mesh).collect();
    assert_eq!(meshes.len(), 3);
    assert_eq!(placed[2].0.mesh, placed[3].0.mesh);
}
