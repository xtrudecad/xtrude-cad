// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport tests: an orbit gesture turns about the point under the
//! cursor. The camera half is in xtrude-core's test_orbit_pivot.

use crate::common;

use common::{add_box, drag, screen, v3, viewport};
use xtrude_app::input::Button;
use xtrude_app::viewport::{NavMode, Viewport};

const W: f64 = 800.0;
const H: f64 = 600.0;

/// A 10 m box at (500, 500), the camera aimed a little beside it.
fn far_box() -> Viewport {
    let mut vp = viewport();
    vp.resize(W, H);
    add_box(&mut vp, v3(495.0, 495.0, 0.0), v3(505.0, 505.0, 10.0));
    let cam = &mut vp.camera;
    cam.aspect = W / H;
    cam.perspective = true;
    cam.target = v3(485.0, 490.0, 0.0);
    cam.distance = 60.0;
    cam.yaw = (-60f64).to_radians();
    cam.pitch = 25f64.to_radians();
    vp
}

#[test]
fn test_the_point_under_the_cursor_stays_put_while_orbiting() {
    let mut vp = far_box();
    let box_top = v3(500.0, 500.0, 10.0);
    let (sx, sy) = screen(&vp, box_top);
    assert!(
        0.0 < sx && sx < W && 0.0 < sy && sy < H,
        "the box must be on screen"
    );
    let pivot = vp.orbit_pivot_at(sx, sy);
    assert!((pivot - box_top).length() < 1.0, "{pivot:?}");
    let before = (vp.camera.yaw, vp.camera.pitch);
    drag(&mut vp, sx, sy, sx + 120.0, sy + 60.0, Button::Middle);
    assert_ne!((vp.camera.yaw, vp.camera.pitch), before, "it did not orbit");
    let (ax, ay) = screen(&vp, pivot);
    assert!(
        (ax - sx).abs() < 1.5 && (ay - sy).abs() < 1.5,
        "{sx} {sy} {ax} {ay}"
    );
    assert!(vp.orbit_pivot().is_none(), "the pivot outlived its gesture");
}

#[test]
fn test_the_orbit_tool_drag_turns_about_the_same_pivot() {
    let mut vp = far_box();
    vp.set_nav_mode(Some(NavMode::Orbit));
    let corner = v3(505.0, 495.0, 10.0);
    let (sx, sy) = screen(&vp, corner);
    let pivot = vp.orbit_pivot_at(sx, sy);
    drag(&mut vp, sx, sy, sx - 90.0, sy + 40.0, Button::Left);
    let (ax, ay) = screen(&vp, pivot);
    assert!((ax - sx).abs() < 1.5 && (ay - sy).abs() < 1.5);
}

#[test]
fn test_over_empty_sky_it_turns_about_the_middle_of_the_model_not_the_origin() {
    let mut vp = far_box();
    vp.camera.target = v3(500.0, 500.0, 5.0);
    let pivot = vp.orbit_pivot_at(5.0, 5.0);
    assert!((pivot - v3(500.0, 500.0, 5.0)).length() < 12.0, "{pivot:?}");
    assert!(pivot.length() > 600.0, "it pivoted on the world origin");
}

#[test]
fn test_an_empty_scene_turns_about_the_target() {
    let mut vp = viewport();
    vp.resize(W, H);
    vp.scene.version += 1;
    vp.camera.target = v3(3.0, 4.0, 5.0);
    assert_eq!(vp.orbit_pivot_at(W / 2.0, H / 2.0), v3(3.0, 4.0, 5.0));
}
