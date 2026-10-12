// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Switching the hidden view off drops the ghosts from the selection.

use crate::common;

use common::window;
use glam::{DMat4, DVec3};
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

#[test]
fn test_switching_the_hidden_view_off_deselects_the_ghost() {
    let mut w = window();
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some("box"));
    g.xform = Some(DMat4::IDENTITY);
    g.hidden = true;
    let gid = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.trigger("show_hidden_groups"); // on
    assert!(w.viewport.scene.show_hidden_objects);
    w.viewport.scene.selection.insert(EntityRef::Group(gid));
    w.trigger("show_hidden_groups"); // off
    assert!(!w.viewport.scene.selection.contains(&EntityRef::Group(gid)));
}
