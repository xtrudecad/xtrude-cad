// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Zoom Extents over the selection; with nothing selected it does nothing.

use crate::common;

use common::{v3, window};
use glam::{DMat4, DVec3};
use xtrude_app::window::{KeySeq, MainWindow};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::scene::{Scene, SelectMode};

fn square(m: &mut Mesh, x0: f64, size: f64) -> FaceId {
    m.add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + size, 0., 0.),
            v3(x0 + size, size, 0.),
            v3(x0, size, 0.),
        ],
        &[],
    )
}

fn corners(s: &Scene) -> (DVec3, DVec3) {
    s.selection_bounds().expect("bounds")
}

#[test]
fn test_nothing_selected_has_no_bounds() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0., 1.);
    assert!(s.selection_bounds().is_none());
}

#[test]
fn test_bounds_cover_only_the_selected_face() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0., 1.);
    let far = square(s.mesh_mut(), 100., 2.);
    s.select([EntityRef::Face(MeshId::LOOSE, far)], SelectMode::Replace);
    assert_eq!(corners(&s), (v3(100., 0., 0.), v3(102., 2., 0.)));
}

#[test]
fn test_bounds_of_a_selected_edge() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0., 1.);
    let e = s.mesh().edge_ids()[0];
    let (a, b) = s.mesh().edge_ab(e);
    s.select([EntityRef::Edge(MeshId::LOOSE, e)], SelectMode::Replace);
    let (lo, hi) = corners(&s);
    assert_eq!(lo.x, a.x.min(b.x));
    assert_eq!(hi.y, a.y.max(b.y));
}

fn group_with(s: &mut Scene, name: &str, x0: f64) -> Group {
    let mut m = Mesh::new();
    square(&mut m, x0, 1.);
    let id = add_mesh(&mut s.doc.meshes, m);
    Group::new(id, Some(name))
}

#[test]
fn test_bounds_of_a_transformed_group() {
    let mut s = Scene::new();
    let mut g = group_with(&mut s, "box", 0.);
    g.xform = Some(DMat4::from_translation(v3(50., 0., 0.)));
    let id = g.id;
    s.doc.groups.push(g);
    s.select([EntityRef::Group(id)], SelectMode::Replace);
    assert_eq!(corners(&s), (v3(50., 0., 0.), v3(51., 1., 0.)));
}

#[test]
fn test_bounds_include_nested_children() {
    let mut s = Scene::new();
    let mut outer = group_with(&mut s, "exterior", 0.);
    let child = group_with(&mut s, "child", 20.);
    outer.children.push(child);
    let id = outer.id;
    s.doc.groups.push(outer);
    s.select([EntityRef::Group(id)], SelectMode::Replace);
    assert_eq!(corners(&s), (v3(0., 0., 0.), v3(21., 1., 0.)));
}

fn win() -> MainWindow {
    let mut w = window();
    square(w.viewport.scene.mesh_mut(), 0., 1.);
    w
}

#[test]
fn test_zoom_selection_frames_the_selection_not_the_model() {
    let mut w = win();
    let far = square(w.viewport.scene.mesh_mut(), 500., 2.);
    w.viewport
        .scene
        .select([EntityRef::Face(MeshId::LOOSE, far)], SelectMode::Replace);
    w.trigger("zoom_selection");
    let cam = &w.viewport.camera;
    assert!((cam.target.x - 501.).abs() < 2.0);
    assert!(cam.distance < 40.0); // not the 500 m model
}

#[test]
fn test_zoom_selection_with_nothing_selected_does_nothing() {
    let mut w = win();
    w.viewport.camera.target = v3(7., 8., 9.);
    w.viewport.camera.distance = 123.0;
    w.trigger("zoom_selection");
    assert_eq!(w.viewport.camera.target, v3(7., 8., 9.));
    assert_eq!(w.viewport.camera.distance, 123.0);
}

#[test]
fn test_the_shortcut_and_the_button_run_the_action() {
    let mut w = win();
    let act = w.actions.get("zoom_selection").unwrap();
    assert!(act.enabled);
    assert_eq!(act.shortcuts.first(), KeySeq::parse("Ctrl+Alt+Z").as_ref());
    let far = square(w.viewport.scene.mesh_mut(), 500., 2.);
    w.viewport
        .scene
        .select([EntityRef::Face(MeshId::LOOSE, far)], SelectMode::Replace);
    w.trigger("zoom_selection");
    assert!((w.viewport.camera.target.x - 501.).abs() < 2.0);
}
