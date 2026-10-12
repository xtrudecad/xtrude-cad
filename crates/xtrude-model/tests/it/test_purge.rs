// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Survey and command tests: Purge Unused sweeps layers and materials nothing
//! wears — nested placements, a group's own paint and the scenes count — as
//! one undo step. The two Layers-panel tests are in xtrude-app.

use crate::common;
use common::*;
use xtrude_core::attrs::Paint;
use xtrude_core::math::DMat4;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::PurgeUnusedCommand;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::history::History;
use xtrude_model::layers::{Layer, DEFAULT_LAYER};
use xtrude_model::materials::Material;
use xtrude_model::purge::{unused_layers, unused_materials, used_layers, used_materials};
use xtrude_model::saved_views::{CameraPose, SavedView};
use xtrude_model::scene::Scene;

fn quad(m: &mut Mesh, x0: f64) -> FaceId {
    m.add_face(&[V(x0, 0.), V(x0 + 2., 0.), V(x0 + 2., 2.), V(x0, 2.)], &[])
}

fn names(s: &Scene) -> Vec<String> {
    s.doc.layers.iter().map(|l| l.name.clone()).collect()
}

fn tag(m: &mut Mesh, f: FaceId, layer: &str) {
    m.face_mut(f).attrs.layer = Some(layer.into());
}

#[test]
fn test_empty_layer_is_unused() {
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0.);
    s.doc.layers.push(Layer::new("CVL_POINT"));
    assert_eq!(unused_layers(&s), vec!["CVL_POINT".to_string()]);
}

#[test]
fn test_layer_worn_by_a_face_survives() {
    let mut s = Scene::new();
    let f = quad(s.mesh_mut(), 0.);
    s.doc.layers.push(Layer::new("OUTLINE"));
    tag(s.mesh_mut(), f, "OUTLINE");
    assert!(used_layers(&s).contains("OUTLINE"));
    assert!(unused_layers(&s).is_empty());
}

#[test]
fn test_the_default_layer_is_never_swept() {
    let s = Scene::new();
    assert_eq!(s.doc.layers[0].name, DEFAULT_LAYER);
    assert!(unused_layers(&s).is_empty());
}

#[test]
fn test_a_layer_used_only_inside_a_group_survives() {
    let mut s = Scene::new();
    let mut inner = Mesh::new();
    let f = quad(&mut inner, 0.);
    tag(&mut inner, f, "Supports");
    let id = add_mesh(&mut s.doc.meshes, inner);
    s.doc.groups.push(Group::new(id, Some("Spacer")));
    s.doc.layers.push(Layer::new("Supports"));
    assert!(unused_layers(&s).is_empty());
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_a_layer_used_only_inside_a_NESTED_placement_survives() {
    let mut s = Scene::new();
    let mut deep = Mesh::new();
    let f = quad(&mut deep, 0.);
    tag(&mut deep, f, "Fittings");
    let did = add_mesh(&mut s.doc.meshes, deep);
    let mut child = Group::new(did, Some("Clip"));
    child.xform = Some(DMat4::IDENTITY);
    let pid = add_mesh(&mut s.doc.meshes, Mesh::new());
    let mut parent = Group::new(pid, Some("Mesa"));
    parent.adopt(vec![child]);
    s.doc.groups.push(parent);
    s.doc.layers.push(Layer::new("Fittings"));
    assert!(used_layers(&s).contains("Fittings"));
    assert!(unused_layers(&s).is_empty());
}

#[test]
fn test_a_layer_a_scene_hides_is_kept() {
    let mut s = Scene::new();
    s.doc.layers.push(Layer::new("Annotations"));
    let mut view = SavedView::new("Top", CameraPose::default());
    view.hidden_layers = vec!["Annotations".into()];
    s.saved_views.push(view);
    assert!(unused_layers(&s).is_empty());
}

#[test]
fn test_purge_removes_and_undo_restores_at_the_same_index() {
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0.);
    for n in ["HOLES", "SLOTS", "boundary"] {
        s.doc.layers.push(Layer::new(n));
    }
    let mut h = History::new();
    h.execute(&mut s, &mut PurgeUnusedCommand::new(true, false));
    assert_eq!(names(&s), vec![DEFAULT_LAYER]);
    assert!(h.undo(&mut s));
    assert_eq!(names(&s), vec![DEFAULT_LAYER, "HOLES", "SLOTS", "boundary"]);
    assert!(h.redo(&mut s));
    assert_eq!(names(&s), vec![DEFAULT_LAYER]);
}

#[test]
fn test_redo_removes_what_the_first_run_chose_not_a_fresh_survey() {
    // A layer created after the purge must not vanish on redo.
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0.);
    s.doc.layers.push(Layer::new("CVL_CURV_D"));
    let mut h = History::new();
    h.execute(&mut s, &mut PurgeUnusedCommand::new(true, false));
    assert!(h.undo(&mut s));
    s.doc.layers.push(Layer::new("Just created"));
    assert!(h.redo(&mut s));
    assert_eq!(names(&s), vec![DEFAULT_LAYER, "Just created"]);
}

fn add_material(s: &mut Scene, name: &str, rgb: [f64; 3]) {
    s.doc
        .materials
        .insert(name.into(), Material::new(name, Some(rgb), None));
}

#[test]
fn test_unused_materials_are_the_ones_no_face_wears() {
    let mut s = Scene::new();
    let f = quad(s.mesh_mut(), 0.);
    add_material(&mut s, "Grey PLA", [0.5, 0.5, 0.5]);
    add_material(&mut s, "Wood_fill", [0.1, 0.4, 0.1]);
    s.mesh_mut().face_mut(f).attrs.paint.mat = Some("Grey PLA".into());
    assert_eq!(
        used_materials(&s),
        ["Grey PLA".to_string()].into_iter().collect()
    );
    assert_eq!(unused_materials(&s), vec!["Wood_fill".to_string()]);
}

#[test]
fn test_a_material_worn_inside_a_group_survives() {
    let mut s = Scene::new();
    let mut inner = Mesh::new();
    let f = quad(&mut inner, 0.);
    inner.face_mut(f).attrs.paint.mat = Some("Marble_07_1K".into());
    let id = add_mesh(&mut s.doc.meshes, inner);
    s.doc.groups.push(Group::new(id, Some("Socket")));
    add_material(&mut s, "Marble_07_1K", [0.9, 0.9, 0.9]);
    assert!(unused_materials(&s).is_empty());
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_a_material_only_a_GROUP_wears_survives() {
    let mut s = Scene::new();
    let mut inner = Mesh::new();
    quad(&mut inner, 0.);
    let id = add_mesh(&mut s.doc.meshes, inner);
    let mut bx = Group::new(id, Some("Box"));
    bx.material = Some(Paint {
        color: Some([0.8, 0.2, 0.2]),
        mat: Some("Red".into()),
        opacity: None,
    });
    let mut cm = Mesh::new();
    quad(&mut cm, 3.0);
    let cid = add_mesh(&mut s.doc.meshes, cm);
    let mut child = Group::new(cid, Some("Child"));
    child.material = Some(Paint {
        color: Some([0.2, 0.2, 0.8]),
        mat: Some("Blue".into()),
        opacity: None,
    });
    let oid = add_mesh(&mut s.doc.meshes, Mesh::new());
    let mut outer = Group::new(oid, Some("Row"));
    outer.adopt(vec![child]);
    s.doc.groups.push(bx);
    s.doc.groups.push(outer);
    add_material(&mut s, "Red", [0.8, 0.2, 0.2]);
    add_material(&mut s, "Blue", [0.2, 0.2, 0.8]);
    add_material(&mut s, "Loose", [0.5, 0.5, 0.5]);
    assert_eq!(
        used_materials(&s),
        ["Red".to_string(), "Blue".to_string()]
            .into_iter()
            .collect()
    );
    assert_eq!(unused_materials(&s), vec!["Loose".to_string()]);
}

#[test]
fn test_purging_materials_is_undoable_with_the_recipe_intact() {
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0.);
    add_material(&mut s, "Brick_Soldier_Bond_1K", [0.7, 0.3, 0.2]);
    let mat = s.doc.materials["Brick_Soldier_Bond_1K"].clone();
    let mut h = History::new();
    h.execute(&mut s, &mut PurgeUnusedCommand::new(false, true));
    assert!(s.doc.materials.is_empty());
    assert!(h.undo(&mut s));
    assert_eq!(s.doc.materials["Brick_Soldier_Bond_1K"], mat);
}

#[test]
fn test_flags_scope_the_sweep() {
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0.);
    s.doc.layers.push(Layer::new("Trees"));
    add_material(&mut s, "grass", [0.2, 0.6, 0.2]);
    let mut h = History::new();
    h.execute(&mut s, &mut PurgeUnusedCommand::new(true, false));
    assert_eq!(names(&s), vec![DEFAULT_LAYER]);
    assert!(s.doc.materials.contains_key("grass"));
}
