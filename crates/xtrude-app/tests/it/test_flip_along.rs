// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Flip Along mirrors the selection in place about its centre, in one undo
//! step; nothing selected, nothing done.

use crate::common;

use common::{v3, window};
use glam::{DMat4, DVec3};
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

/// World positions of a group's vertices.
fn world(w: &MainWindow, g: GroupId) -> Vec<DVec3> {
    let grp = w.viewport.scene.doc.find_group(g).unwrap();
    let x = grp.xform.unwrap_or(DMat4::IDENTITY);
    w.viewport
        .scene
        .doc
        .mesh(grp.mesh)
        .verts
        .iter()
        .map(|(_, v)| x.transform_point3(v.position))
        .collect()
}

fn r3(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

fn xs(ps: &[DVec3]) -> Vec<i64> {
    let mut out: Vec<i64> = ps.iter().map(|p| r3(p.x)).collect();
    out.sort();
    out
}

fn ys_at_x(ps: &[DVec3], x: f64) -> Vec<i64> {
    let mut out: Vec<i64> = ps
        .iter()
        .filter(|p| r3(p.x) == r3(x))
        .map(|p| r3(p.y))
        .collect();
    out.sort();
    out
}

#[test]
fn test_flip_along_red_mirrors_a_group_in_place_in_one_undo() {
    let mut w = window();
    let mut m = Mesh::new();
    m.add_face(&[v3(1., 0., 0.), v3(3., 0., 0.), v3(1., 1., 0.)], &[]); // thick on the left
    let g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), None);
    let gid = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.selection.insert(EntityRef::Group(gid));
    assert_eq!(xs(&world(&w, gid)), vec![1000, 1000, 3000]);
    w.trigger("flip_along:x");
    // Mirrored about x = 2: the thick end is now on the right.
    let ps = world(&w, gid);
    assert_eq!(xs(&ps), vec![1000, 3000, 3000]); // same span, in place
    assert_eq!(ys_at_x(&ps, 3.0), vec![0, 1000]); // the tall side moved right
    assert!(w.viewport.undo());
    assert_eq!(ys_at_x(&world(&w, gid), 1.0), vec![0, 1000]); // back as it was
}

#[test]
fn test_flip_along_blue_turns_loose_geometry_upside_down() {
    let mut w = window();
    let f = w
        .viewport
        .scene
        .mesh_mut()
        .add_face(&[v3(0., 0., 0.), v3(1., 0., 0.), v3(1., 0., 2.)], &[]);
    w.viewport
        .scene
        .selection
        .insert(EntityRef::Face(MeshId::LOOSE, f));
    w.trigger("flip_along:z");
    let m = w.viewport.scene.mesh();
    let face = m.face_ids()[0];
    let mut zs: Vec<i64> = m.face_positions(face).iter().map(|p| r3(p.z)).collect();
    zs.sort();
    assert_eq!(zs, vec![0, 2000, 2000]); // mirrored about z = 1
}

#[test]
fn test_nothing_selected_changes_nothing() {
    let mut w = window();
    let n = w.viewport.history.undo_len();
    w.trigger("flip_along:y");
    assert_eq!(w.viewport.history.undo_len(), n);
}
