// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rebuild Faces (planar) on the selected faces rebuilds their plane in a 3D
//! model and leaves the rest alone; with nothing selected the
//! whole-flat-drawing rule stands.

use crate::common;

use common::{v3, window};
use xtrude_app::window::MainWindow;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn square(m: &mut Mesh, x: f64, y: f64, s: f64) -> FaceId {
    m.add_face(
        &[
            v3(x, y, 0.),
            v3(x + s, y, 0.),
            v3(x + s, y + s, 0.),
            v3(x, y + s, 0.),
        ],
        &[],
    )
}

/// A 1 m box floating 1 m up.
fn raised_box(m: &mut Mesh) {
    let mut c = Vec::new();
    for z in [1., 2.] {
        for y in [5., 6.] {
            for x in [5., 6.] {
                c.push(v3(x, y, z));
            }
        }
    }
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        m.add_face(&q.map(|i| c[i]), &[]);
    }
}

fn ground(m: &Mesh) -> usize {
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.z.abs() < 1e-6))
        .count()
}

fn scene() -> (MainWindow, FaceId) {
    let mut w = window();
    let m = w.viewport.scene.mesh_mut();
    raised_box(m);
    let a = square(m, 0., 0., 2.);
    square(m, 1., 1., 2.); // overlaps the first one
    w.viewport.scene.version += 1;
    (w, a)
}

fn select(w: &mut MainWindow, faces: &[FaceId]) {
    w.viewport.scene.clear_selection();
    for &f in faces {
        w.viewport
            .scene
            .selection
            .insert(EntityRef::Face(MeshId::LOOSE, f));
    }
}

fn faces(w: &MainWindow) -> usize {
    w.viewport.scene.mesh().face_ids().len()
}

#[test]
fn test_the_selected_plane_is_rebuilt_in_a_3d_model() {
    let (mut w, a) = scene();
    let box_before = faces(&w) - 2;
    select(&mut w, &[a]);
    w.trigger("rebuild_planar");
    let g = ground(w.viewport.scene.mesh());
    assert_eq!(g, 3); // two overlapping squares → three regions
    assert_eq!(faces(&w) - g, box_before); // the box untouched
}

#[test]
fn test_faces_on_different_planes_are_refused() {
    let (mut w, a) = scene();
    let m = w.viewport.scene.mesh();
    let top = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - 2.0).abs() < 1e-6))
        .unwrap();
    let before = faces(&w);
    select(&mut w, &[a, top]);
    w.trigger("rebuild_planar");
    w.pump_events();
    assert_eq!(faces(&w), before);
    let msg = w
        .status_message
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default();
    assert!(msg.contains("one plane"), "{msg}");
}

#[test]
fn test_nothing_selected_keeps_the_flat_only_rule() {
    let (mut w, _a) = scene();
    let before = faces(&w);
    w.viewport.scene.clear_selection();
    w.trigger("rebuild_planar");
    assert_eq!(faces(&w), before); // a 3D model: left alone
}

#[test]
fn test_one_undo_puts_the_plane_back() {
    let (mut w, a) = scene();
    let before = ground(w.viewport.scene.mesh());
    select(&mut w, &[a]);
    w.trigger("rebuild_planar");
    w.viewport.undo();
    assert_eq!(ground(w.viewport.scene.mesh()), before);
}
