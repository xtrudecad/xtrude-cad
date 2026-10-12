// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A loose line lying on a group's face is picked before the group; a line
//! hidden behind the group does not steal the click.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{v3, window};
use xtrude_app::tools::select;
use xtrude_app::window::MainWindow;
use xtrude_core::{EdgeId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn setup() -> (MainWindow, GroupId, EdgeId, EdgeId) {
    let mut w = window();
    let vp = &mut w.viewport;
    let p = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
        v3(0., 0., 1.),
        v3(1., 0., 1.),
        v3(1., 1., 1.),
        v3(0., 1., 1.),
    ];
    let mut m = Mesh::new();
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        m.add_face(&q.map(|i| p[i]), &[]);
    }
    let bx = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some("Block"));
    let bid = bx.id;
    vp.scene.doc.groups.push(bx);
    // a loose line lying ON the box's top face, and one hidden underneath
    let lm = vp.scene.mesh_mut();
    let on_top = lm.add_edge(v3(0.2, 0.5, 1.0), v3(0.8, 0.5, 1.0)).unwrap();
    let below = lm.add_edge(v3(0.2, 0.5, -0.3), v3(0.8, 0.5, -0.3)).unwrap();
    vp.scene.version += 1;
    vp.resize(800., 600.);
    vp.camera.set_aspect(800, 600);
    let c = &mut vp.camera;
    c.perspective = true;
    c.yaw = 20f64.to_radians();
    c.pitch = 55f64.to_radians(); // from above
    c.target = v3(0.5, 0.5, 0.5);
    c.distance = 4.0;
    (w, bid, on_top, below)
}

#[test]
fn test_a_line_on_a_group_face_is_picked_before_the_group() {
    let (mut w, bx, on_top, _below) = setup();
    let vp = &mut w.viewport;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(0.5, 0.5, 1.0)).unwrap(); // on the line
    assert_eq!(vp.pick_group(x, y), Some(bx)); // the group IS there
    assert_eq!(
        select::pick(vp, x, y),
        Some(EntityRef::Edge(MeshId::LOOSE, on_top))
    ); // …the line wins
    let ScreenPt { x: x2, y: y2 } = vp.world_to_pixel(v3(0.5, 0.15, 1.0)).unwrap(); // the face, away from it
    assert_eq!(select::pick(vp, x2, y2), Some(EntityRef::Group(bx)));
}

#[test]
fn test_a_line_hidden_behind_the_group_does_not_steal_the_click() {
    let (mut w, bx, on_top, below) = setup();
    let vp = &mut w.viewport;
    vp.scene.mesh_mut().remove_edge(on_top);
    vp.scene.version += 1;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(0.5, 0.5, -0.3)).unwrap(); // over the hidden line
    let pick = select::pick(vp, x, y);
    assert!(pick == Some(EntityRef::Group(bx)) || !matches!(pick, Some(EntityRef::Edge(..))));
    let (a, b) = vp.scene.mesh().edge_ab(below);
    let p = vp
        .edge_point_under_cursor(a, b, x, y)
        .expect("a point on it");
    assert!(vp.is_occluded(p));
}
