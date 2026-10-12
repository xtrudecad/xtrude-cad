// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The pan moves by the depth of the grabbed point.

use xtrude_core::camera::{OrbitCamera, MIN_DISTANCE};
use xtrude_core::{DVec3, DVec4};

const W: f64 = 1000.0;
const H: i32 = 700;

fn screen_x(cam: &OrbitCamera, p: DVec3) -> f64 {
    let clip = cam.projection_matrix() * cam.view_matrix() * DVec4::new(p.x, p.y, p.z, 1.0);
    (clip.x / clip.w * 0.5 + 0.5) * W
}

fn cam(perspective: bool) -> OrbitCamera {
    let mut cam = OrbitCamera::new();
    cam.set_aspect(W as i32, H);
    cam.perspective = perspective;
    cam.target = DVec3::ZERO;
    cam.distance = MIN_DISTANCE;
    cam
}

#[test]
fn test_the_grabbed_point_follows_the_cursor_at_full_zoom() {
    let mut cam = cam(true);
    let right = cam.forward().cross(cam.up_vector()).normalize();
    let wall = cam.eye() + cam.forward() * 5.0 + right * 0.5;
    let depth = (wall - cam.eye()).dot(cam.forward());
    let x0 = screen_x(&cam, wall);
    cam.pan(100.0, 0.0, H, Some(depth));
    assert!((screen_x(&cam, wall) - x0 - 100.0).abs() <= 0.5);
}

#[test]
fn test_without_a_depth_it_was_a_crawl() {
    let mut cam = cam(true);
    let wall = cam.eye() + cam.forward() * 5.0;
    let x0 = screen_x(&cam, wall);
    cam.pan(100.0, 0.0, H, None);
    assert!((screen_x(&cam, wall) - x0).abs() < 1.0);
}

#[test]
fn test_parallel_views_ignore_the_depth() {
    let (mut a, mut b) = (cam(false), cam(false));
    a.distance = 10.0;
    b.distance = 10.0;
    a.pan(100.0, 40.0, H, None);
    b.pan(100.0, 40.0, H, Some(250.0));
    assert!((a.target - b.target).length() < 1e-9);
}

#[test]
fn test_a_useless_depth_falls_back_to_the_target() {
    for bad in [
        None,
        Some(0.0),
        Some(-3.0),
        Some(f64::INFINITY),
        Some(f64::NAN),
    ] {
        let (mut a, mut b) = (cam(true), cam(true));
        a.distance = 4.0;
        b.distance = 4.0;
        a.pan(50.0, 20.0, H, None);
        b.pan(50.0, 20.0, H, bad);
        assert!((a.target - b.target).length() < 1e-9, "{bad:?}");
    }
}
