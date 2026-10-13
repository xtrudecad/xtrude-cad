// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print materials in the document (format 2): the document's and the
//! groups' own are saved and read back; a format-1 document opens with
//! every group in PLA; a newer format is refused.

use crate::common;
use common::*;
use std::path::Path;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_io::document::{self, DocError, FORMAT};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::print_material::{default_print_material, PrintMaterial};
use xtrude_model::scene::Scene;

fn square_group(s: &mut Scene, name: &str) -> Group {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    Group::new(add_mesh(&mut s.doc.meshes, m), Some(name))
}

/// A document default, a group of its own, a nested group of its own
/// inside a group without, two copies of a component, and a group
/// without.
fn materials_scene() -> Scene {
    let mut s = Scene::new();
    s.doc.print_material = PrintMaterial::new("Resin", 1100.0);
    let mut own = square_group(&mut s, "own");
    own.print_material = PrintMaterial::built_in("PETG");
    let mut inner = square_group(&mut s, "inner");
    inner.print_material = PrintMaterial::built_in("TPU");
    let mut outer = square_group(&mut s, "outer");
    outer.adopt(vec![inner]);
    let mut comp = square_group(&mut s, "comp");
    comp.xform = Some(DMat4::IDENTITY);
    comp.print_material = PrintMaterial::built_in("PC");
    let mut copy = comp.clone();
    copy.id = xtrude_model::ids::next_id();
    copy.xform = Some(DMat4::from_translation(DVec3::X * 2.0));
    let plain = square_group(&mut s, "plain");
    s.doc.groups = vec![own, outer, comp, copy, plain];
    s
}

fn materials_by_name(s: &Scene) -> Vec<(String, Option<PrintMaterial>)> {
    s.doc
        .all_groups()
        .iter()
        .map(|g| (g.name.clone(), g.print_material.clone()))
        .collect()
}

fn load(path: &Path) -> Result<Scene, DocError> {
    let mut s = Scene::new();
    document::load_into(&mut s, path, None).map(|_| s)
}

#[test]
fn test_the_format_is_2() {
    assert_eq!(FORMAT, 2);
}

#[test]
fn test_print_materials_survive_a_save_and_reopen() {
    let s = materials_scene();
    let tmp = TmpDir::new();
    let path = tmp.join("materials.xtrude");
    document::save_scene(&s, &path).unwrap();
    let p = payload(&path);
    assert_eq!(
        p["print_material"],
        serde_json::json!({"name": "Resin", "density": 1100.0})
    );
    assert_eq!(
        p["groups"][0]["print_material"],
        serde_json::json!({"name": "PETG", "density": 1270.0})
    );
    assert!(p["groups"][4].get("print_material").is_none());
    let back = load(&path).unwrap();
    assert_eq!(back.doc.print_material, s.doc.print_material);
    assert_eq!(materials_by_name(&back), materials_by_name(&s));
}

#[test]
fn test_a_format_1_document_opens_in_pla() {
    let s = materials_scene();
    let tmp = TmpDir::new();
    let path = tmp.join("old.xtrude");
    document::save_scene(&s, &path).unwrap();
    // what a 0.1.0 build wrote: format 1, no print material anywhere
    let mut doc = document_json(&path);
    doc["xtrude_format"] = 1.into();
    let scene = doc["scene"].as_object_mut().unwrap();
    scene.remove("print_material");
    for g in scene["groups"].as_array_mut().unwrap() {
        g.as_object_mut().unwrap().remove("print_material");
        for c in g
            .get_mut("children")
            .and_then(|c| c.as_array_mut())
            .into_iter()
            .flatten()
        {
            c.as_object_mut().unwrap().remove("print_material");
        }
    }
    assert!(!serde_json::to_string(&doc)
        .unwrap()
        .contains("print_material"));
    write_document_json(&path, &doc);
    let back = load(&path).unwrap();
    assert_eq!(back.doc.print_material, default_print_material());
    assert!(back
        .doc
        .all_groups()
        .iter()
        .all(|g| g.print_material.is_none()));
    assert_eq!(back.doc.all_groups().len(), 6);
}

#[test]
fn test_a_format_3_document_is_refused() {
    let tmp = TmpDir::new();
    let path = tmp.join("future.xtrude");
    document::save_scene(&materials_scene(), &path).unwrap();
    let mut doc = document_json(&path);
    doc["xtrude_format"] = 3.into();
    write_document_json(&path, &doc);
    assert!(matches!(load(&path), Err(DocError::TooNew(3))));
}

#[test]
fn test_a_print_material_that_is_not_valid_reads_as_none() {
    let tmp = TmpDir::new();
    let path = tmp.join("bad.xtrude");
    document::save_scene(&materials_scene(), &path).unwrap();
    let mut doc = document_json(&path);
    doc["scene"]["print_material"] = serde_json::json!({"name": "Air", "density": 0.0});
    doc["scene"]["groups"][0]["print_material"] = serde_json::json!({"name": "", "density": 1.0});
    doc["scene"]["groups"][4]["print_material"] = serde_json::json!("PLA");
    write_document_json(&path, &doc);
    let back = load(&path).unwrap();
    assert_eq!(back.doc.print_material, default_print_material());
    assert!(back.doc.groups[0].print_material.is_none());
    assert!(back.doc.groups[4].print_material.is_none());
}
