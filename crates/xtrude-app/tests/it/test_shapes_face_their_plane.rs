// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A shape comes out facing the way its plane does, whichever way it is
//! dragged: none of the four rectangle diagonals gives a face pointing
//! backwards.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{click, window, MetreDocument};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::{face_the_plane, loop_normal};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// Draw `points` with the tool `key` through real clicks; the first
/// face's normal.
fn draw(key: &str, points: &[DVec3]) -> Option<DVec3> {
    let mut w = window();
    w.viewport.resize(1000., 700.);
    w.viewport.camera.set_aspect(1000, 700);
    w.activate_tool(key);
    let vp = &mut w.viewport;
    for &p in points {
        let ScreenPt { x, y } = vp.world_to_pixel(p).expect("on screen");
        vp.input.last_mouse_pos = Some(ScreenPt { x, y });
        vp.refresh_snap();
        click(vp, x, y, Modifiers::NONE);
    }
    let m = vp.scene.mesh();
    m.face_ids().first().map(|&f| m.face_normal(f))
}

#[test]
fn test_a_rectangle_on_the_ground_faces_up_from_every_diagonal() {
    let _m = MetreDocument::new();
    for (a, b) in [
        (v(1., 3.), v(4., 1.)), // right-down — was blue
        (v(4., 1.), v(1., 3.)), // left-up    — was blue
        (v(1., 1.), v(4., 3.)), // right-up
        (v(4., 3.), v(1., 1.)), // left-down
    ] {
        let n = draw("rectangle", &[a, b]).expect("a face");
        assert!(n.z > 0.9, "{a} → {b}: {n}");
    }
}

#[test]
fn test_the_rotated_rectangle_too_going_either_way_round() {
    let _m = MetreDocument::new();
    for third in [v(4., 3.), v(4., -1.)] {
        let n = draw("rotated_rect", &[v(1., 1.), v(4., 1.), third]).expect("a face");
        assert!(n.z > 0.9, "{third}: {n}");
    }
}

#[test]
fn test_the_circle_was_already_right() {
    let n = draw("circle", &[v(2., 2.), v(0., 0.)]).expect("a face");
    assert!(n.z > 0.9);
}

#[test]
fn test_a_loop_square_to_its_plane_is_left_alone() {
    let de_pie = vec![
        DVec3::new(0., 0., 0.),
        DVec3::new(2., 0., 0.),
        DVec3::new(2., 0., 1.),
        DVec3::new(0., 0., 1.),
    ];
    assert_eq!(face_the_plane(&de_pie, Some(DVec3::Z)), de_pie);
}

#[test]
fn test_the_helper_reverses_only_what_points_away() {
    let plane = DVec3::Z;
    let facing = vec![v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)];
    assert!(loop_normal(&facing).dot(plane) > 0.0);
    assert_eq!(face_the_plane(&facing, Some(plane)), facing);
    let reversed: Vec<DVec3> = facing.iter().rev().copied().collect();
    assert_ne!(face_the_plane(&reversed, Some(plane)), reversed);
    assert!(loop_normal(&face_the_plane(&reversed, Some(plane))).dot(plane) > 0.0);
}
