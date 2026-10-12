// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A group of nothing but lines and arcs reaches the pick index, so inference
//! sees its edges and a click on one of its lines selects it.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{v3, window};
use glam::DVec3;
use std::f64::consts::PI;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::{compute_snap, SnapKind, SnapQuery};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

/// The first line's midpoint — where the cursor goes.
const ON_THE_LINE: DVec3 = DVec3::new(1.0, 0.0, 0.0);

/// Two straight lines and a quarter arc, not one face among them.
fn lines_and_arcs() -> Mesh {
    let mut m = Mesh::new();
    m.add_edge(v3(0., 0., 0.), v3(2., 0., 0.)).unwrap();
    m.add_edge(v3(2., 0., 0.), v3(2., 2., 0.)).unwrap();
    for i in 0..8 {
        let (a0, a1) = (i as f64 * PI / 16.0, (i + 1) as f64 * PI / 16.0);
        m.add_edge(
            v3(2. + a0.cos(), 2. + a0.sin(), 0.),
            v3(2. + a1.cos(), 2. + a1.sin(), 0.),
        )
        .unwrap();
    }
    m
}

fn setup() -> (MainWindow, GroupId) {
    let mut w = window();
    let vp = &mut w.viewport;
    let g = Group::new(
        add_mesh(&mut vp.scene.doc.meshes, lines_and_arcs()),
        Some("detail"),
    );
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.resize(800., 600.);
    vp.camera.set_aspect(800, 600);
    let c = &mut vp.camera;
    c.yaw = (-45f64).to_radians();
    c.pitch = 35f64.to_radians();
    c.target = v3(1., 1., 0.);
    c.distance = 8.0;
    (w, id)
}

#[test]
fn test_its_edges_reach_the_pick_index() {
    let (mut w, g) = setup();
    let index = w.viewport.pick_index();
    assert_eq!(index.group_edge_count(), 10); // 2 lines + 8 arc segments
    assert!(index.group_edge_owners().iter().any(|&(o, _)| o == g));
}

#[test]
fn test_inference_sees_them() {
    let (mut w, _g) = setup();
    let vp = &mut w.viewport;
    let ScreenPt { x, y } = vp.world_to_pixel(ON_THE_LINE).unwrap();
    assert!(
        !vp.nearby_group_edges(x, y, 48.0, 48).is_empty(),
        "no edge to snap to"
    );
}

#[test]
fn test_an_endpoint_of_the_group_actually_snaps() {
    let (mut w, _g) = setup();
    let vp = &mut w.viewport;
    let corner = v3(2., 0., 0.); // where the two lines meet
    let px = vp.world_to_pixel(corner).unwrap();
    let aim = (px.x + 4.0, px.y + 4.0); // a few pixels off, as a hand does
    let world = vp.world_from_pixel(aim.0, aim.1).expect("a work plane");
    let edges = vp.snap_scene(Some(aim));
    let vp = &w.viewport;
    let w2p = |p: DVec3| vp.world_to_pixel(p).map(|p| p.tuple());
    let q = SnapQuery::new(world, aim, &w2p, vp.prefs.snap_threshold_px);
    let snap = compute_snap(&q, &edges);
    assert_eq!(
        snap.kind,
        SnapKind::Endpoint,
        "nothing to snap to on the group"
    );
    assert!((snap.point - corner).length() < 1e-6);
}

#[test]
fn test_clicking_one_of_its_lines_selects_it() {
    let (mut w, g) = setup();
    let vp = &mut w.viewport;
    let ScreenPt { x, y } = vp.world_to_pixel(ON_THE_LINE).unwrap();
    assert_eq!(vp.pick_group(x, y), Some(g));
}

#[test]
fn test_a_group_with_faces_is_unaffected() {
    let (mut w, _g) = setup();
    let vp = &mut w.viewport;
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(5., 0., 0.),
            v3(6., 0., 0.),
            v3(6., 1., 0.),
            v3(5., 1., 0.),
        ],
        &[],
    );
    let solid = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some("with faces"));
    let sid = solid.id;
    vp.scene.doc.groups.push(solid);
    vp.scene.version += 1;
    let index = vp.pick_index();
    assert!(index.face_count() > 0 && index.placements().iter().any(|p| p.owner == sid));
    if let Some(ScreenPt { x, y }) = vp.world_to_pixel(v3(5.5, 0.5, 0.)) {
        assert_eq!(vp.pick_group(x, y), Some(sid));
    }
}
