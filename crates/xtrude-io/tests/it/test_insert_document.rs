// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Inserting a saved document.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::document;
use xtrude_model::group::{add_mesh, placement_points, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::insert::import_document_as_component;
use xtrude_model::scene::Scene;

fn square(x0: f64, side: f64, z: f64) -> Mesh {
    let mut m = Mesh::new();
    quad(&mut m, x0, x0 + side, 0., side, z);
    m
}

#[test]
fn test_the_saved_file_comes_in_as_a_component() {
    let mut src = Scene::new();
    src.doc
        .meshes
        .insert(MeshId::LOOSE, std::sync::Arc::new(square(0., 3., 0.)));
    let peg = Group::new(
        add_mesh(&mut src.doc.meshes, square(0., 0.2, 0.)),
        Some("Peg"),
    );
    let mut lid = Group::new(
        add_mesh(&mut src.doc.meshes, square(0., 3., 2.4)),
        Some("Lid"),
    );
    lid.xform = Some(DMat4::IDENTITY);
    src.doc.groups.extend([peg, lid]);
    let tmp = TmpDir::new();
    let path = tmp.join("bracket.xtrude");
    document::save_scene(&src, &path).unwrap();
    let mut temp = Scene::new();
    document::load_into(&mut temp, &path, None).unwrap();
    let mut dst = Scene::new();
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let comp = import_document_as_component(&mut dst.doc, temp.doc.clone(), &stem).unwrap();
    assert_eq!(comp.name, "bracket");
    assert_eq!(
        comp.children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Peg", "Lid"]
    );
    assert_eq!(placement_points(&dst.doc.meshes, &comp).len(), 12);
}

#[test]
fn test_the_document_keeps_the_group_names() {
    let mut src = Scene::new();
    let a = Group::new(
        add_mesh(&mut src.doc.meshes, square(0., 1., 0.)),
        Some("Bracket"),
    );
    let auto = Group::new(add_mesh(&mut src.doc.meshes, square(5., 1., 0.)), None);
    let auto_name = auto.name.clone();
    src.doc.groups.extend([a, auto]);
    let back = roundtrip(&src);
    assert_eq!(
        back.doc
            .groups
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>(),
        ["Bracket".to_string(), auto_name.clone()]
    );
    let fresh = Group::new(MeshId::LOOSE, None);
    assert_ne!(fresh.name, auto_name);
    let n_auto: u64 = auto_name
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let n_new: u64 = fresh
        .name
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(n_new > n_auto);
}
