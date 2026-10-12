// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A part without a matrix in a component's definition (a child with no
//! `xform` and the default `component` flag) stays a part while a copy of
//! the component is open, and comes back as it was on leaving.

use crate::common;

use common::{v3, window};
use glam::DMat4;
use xtrude_app::tray::components::Components;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::commands::MoveGroupCommand;
use xtrude_model::group::{add_mesh, copy_group, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::pushpull::PushPull;

/// A unit square at height `z`, in its own mesh.
fn square_mesh(w: &mut MainWindow, z: f64) -> MeshId {
    let mut m = Mesh::new();
    m.add_face(
        &[v3(0., 0., z), v3(1., 0., z), v3(1., 1., z), v3(0., 1., z)],
        &[],
    );
    add_mesh(&mut w.viewport.scene.doc.meshes, m)
}

/// A "Bracket" component placed twice: (first, second). The first holds a
/// "Bolt" part without a matrix, as `Group::new` makes it; the second, a
/// copy of it, holds the bolt placed at identity.
fn bracket_twice(w: &mut MainWindow) -> (GroupId, GroupId) {
    let body = square_mesh(w, 0.);
    let bolt = square_mesh(w, 2.);
    let mut a = Group::new(body, Some("Bracket"));
    a.xform = Some(DMat4::IDENTITY);
    a.children = vec![Group::new(bolt, Some("Bolt"))];
    assert!(a.children[0].xform.is_none() && a.children[0].component);
    let ia = a.id;
    let doc = &mut w.viewport.scene.doc;
    let mut dup = copy_group(&mut doc.meshes, &mut a.clone(), None, false);
    dup.xform = Some(DMat4::from_translation(v3(10., 0., 0.)));
    let ib = dup.id;
    doc.groups.push(a);
    doc.groups.push(dup);
    w.viewport.scene.version += 1;
    (ia, ib)
}

fn g(w: &MainWindow, id: GroupId) -> &Group {
    w.viewport.scene.doc.find_group(id).unwrap()
}

fn components(w: &MainWindow) -> Vec<String> {
    let mut c = Components::default();
    c.refresh_in_model(w);
    c.rows.into_iter().map(|r| r.label).collect()
}

#[test]
fn test_a_part_without_a_matrix_is_no_component_while_its_copy_is_open() {
    let mut w = window();
    let (a, _b) = bracket_twice(&mut w);
    assert_eq!(components(&w).len(), 1, "{:?}", components(&w));
    w.viewport.begin_group_edit(a);
    let bolt = &g(&w, a).children[0];
    assert!(!bolt.is_component(), "the bolt is a part, not a copy");
    let rows = components(&w);
    assert_eq!(rows.len(), 1, "only the bracket is listed: {rows:?}");
    assert!(rows[0].starts_with("Bracket"), "{rows:?}");
}

#[test]
fn test_a_look_inside_leaves_the_part_as_it_was() {
    let mut w = window();
    let (a, b) = bracket_twice(&mut w);
    let before = (g(&w, a).clone(), g(&w, b).clone());
    w.viewport.begin_group_edit(a);
    let bolt = g(&w, a).children[0].id;
    w.viewport.begin_group_edit(bolt);
    w.viewport.end_group_edit();
    assert_eq!(
        g(&w, a),
        &before.0,
        "matrix and flag of the bolt as they were"
    );
    assert_eq!(g(&w, b), &before.1);
    assert_eq!(w.viewport.history.undo_len(), 0);
}

#[test]
fn test_an_edit_inside_still_reaches_the_other_copy() {
    let mut w = window();
    let (a, b) = bracket_twice(&mut w);
    let bolt0 = g(&w, a).children[0].clone();
    w.viewport.begin_group_edit(a);
    let target = w.viewport.scene.active_mesh_id();
    let face = w.viewport.scene.mesh().face_ids()[0];
    let mut pp = PushPull::new();
    pp.setup_plain(w.viewport.scene.mesh(), target, face);
    pp.extrusion = 1.0;
    pp.commit(&mut w.viewport.scene, &mut w.viewport.history);
    w.viewport.end_group_edit();
    let faces = |w: &MainWindow, id| w.viewport.scene.doc.mesh(g(w, id).mesh).faces.len();
    assert!(faces(&w, b) > 1, "the pushed face reached the other copy");
    // An edit shares the contents: the bolt gets its place in the
    // definition, as a part, in every copy.
    for id in [a, b] {
        let kids = &g(&w, id).children;
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].mesh, bolt0.mesh);
        assert_eq!(kids[0].xform, Some(DMat4::IDENTITY));
        assert!(!kids[0].is_component(), "a part in every copy");
    }
    assert_eq!(components(&w).len(), 1, "{:?}", components(&w));
}

#[test]
fn test_a_part_moved_inside_stays_a_part_in_every_copy() {
    let mut w = window();
    let (a, b) = bracket_twice(&mut w);
    w.viewport.begin_group_edit(a);
    let bolt = g(&w, a).children[0].id;
    assert!(w.viewport.run(MoveGroupCommand {
        group: bolt,
        delta: v3(0., 0., 1.),
    }));
    w.viewport.end_group_edit();
    for id in [a, b] {
        let kids = &g(&w, id).children;
        assert_eq!(kids.len(), 1);
        assert!(!kids[0].is_component(), "a part in every copy");
        let z = kids[0].xform.unwrap().w_axis.z;
        assert!(
            (z - 1.).abs() < 1e-9,
            "moved up by 1 in every copy, not {z}"
        );
    }
    assert_eq!(components(&w).len(), 1, "{:?}", components(&w));
}
