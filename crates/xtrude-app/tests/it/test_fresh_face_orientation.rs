// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Fresh faces look up, or at the eye that drew them.

use crate::common;

use common::{sized_viewport, v3};
use glam::DVec3;
use xtrude_core::math::newell;
use xtrude_model::edits::{build_add_edge, build_add_edges, face_up_or_toward};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn ring_segs(ring: &[DVec3]) -> Vec<(DVec3, DVec3)> {
    (0..ring.len())
        .map(|i| (ring[i], ring[(i + 1) % ring.len()]))
        .collect()
}

#[test]
fn test_a_square_closed_clockwise_with_line_still_faces_up() {
    let mut vp = common::stub();
    for (a, b) in ring_segs(&[v(0., 0.), v(0., 2.), v(2., 2.), v(2., 0.)]) {
        let cmd = build_add_edge(&vp.scene, a, b, true, None);
        vp.execute_boxed(cmd);
    }
    let m = vp.scene.mesh();
    let fs = m.face_ids();
    assert_eq!(fs.len(), 1);
    assert!(m.face_normal(fs[0]).z > 0.9);
}

#[test]
fn test_offsets_ring_faces_up_whatever_the_walk_order() {
    let mut vp = common::stub();
    let segs = ring_segs(&[v(0., 0.), v(0., 3.), v(3., 3.), v(3., 0.)]);
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    let m = vp.scene.mesh();
    let fs = m.face_ids();
    assert_eq!(fs.len(), 1);
    assert!(m.face_normal(fs[0]).z > 0.9);
}

#[test]
fn test_a_vertical_face_looks_at_the_eye_that_drew_it() {
    let wall = vec![
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 0., 2.),
        v3(0., 0., 2.),
    ];
    assert!(newell(&face_up_or_toward(&wall, Some(v3(1., -5., 1.)))).y < 0.0);
    assert!(newell(&face_up_or_toward(&wall, Some(v3(1., 5., 1.)))).y > 0.0);
    assert_eq!(face_up_or_toward(&wall, None), wall); // nothing decides
}

#[test]
fn test_line_closing_a_wall_in_front_of_the_camera_shows_its_front() {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("front"); // the camera at −Y looking +Y
    let eye = vp.camera.eye();
    let pts = [
        v3(0., 0., 0.),
        v3(0., 0., 2.),
        v3(2., 0., 2.),
        v3(2., 0., 0.),
    ];
    for (a, b) in ring_segs(&pts) {
        // what the Line's commit does: the eye decides a vertical face
        let cmd = build_add_edge(&vp.scene, a, b, true, Some(eye));
        vp.execute_boxed(cmd);
    }
    let m = vp.scene.mesh();
    let fs = m.face_ids();
    assert_eq!(fs.len(), 1);
    assert!(m.face_normal(fs[0]).dot(eye - v3(1., 0., 1.)) > 0.0);
}
