// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Wheel zoom never feels stuck at the 2 cm close-up.

use xtrude_core::camera::{OrbitCamera, MIN_DISTANCE};
use xtrude_core::DVec3;

fn close_up(focus: DVec3) -> OrbitCamera {
    let mut cam = OrbitCamera::new();
    cam.perspective = true;
    cam.yaw = 30f64.to_radians();
    cam.pitch = 20f64.to_radians();
    cam.target = focus;
    cam.distance = MIN_DISTANCE;
    cam
}

#[test]
fn test_zooming_out_from_the_minimum_distance_retreats_perceptibly() {
    let focus = DVec3::new(3.0, 2.0, 1.0);
    let mut cam = close_up(focus);
    let eye0 = cam.eye();
    cam.zoom_to(-1.0, focus, 0.0);
    assert!((cam.eye() - eye0).length() < 0.01);
    let mut cam = close_up(focus);
    cam.zoom_to(-1.0, focus, 0.2);
    assert!((cam.eye() - eye0).length() >= 0.2 - 1e-5);
    for _ in 0..10 {
        cam.zoom_to(-1.0, focus, 0.2);
    }
    assert!((cam.eye() - focus).length() > 2.0);
}

#[test]
fn test_zooming_in_at_the_minimum_keeps_sliding_toward_the_focus() {
    let focus = DVec3::new(3.0, 2.0, 1.0);
    let mut cam = close_up(DVec3::new(3.5, 2.0, 1.0));
    cam.zoom_to(1.0, focus, 0.0);
    assert_eq!(cam.distance, MIN_DISTANCE);
    assert!(((cam.target - focus).length() - 0.45).abs() < 1e-5);
}

#[test]
fn test_a_normal_zoom_keeps_the_focus_fixed_and_ignores_the_floor() {
    let focus = DVec3::ZERO;
    let mut cam = OrbitCamera::new();
    cam.target = DVec3::new(1.0, 0.0, 0.0);
    cam.distance = 10.0;
    let eye0 = cam.eye();
    cam.zoom_to(1.0, focus, 0.2);
    assert!((cam.distance - 9.0).abs() < 1e-9);
    assert!((cam.target - focus * 0.1 - DVec3::new(0.9, 0.0, 0.0)).length() < 1e-5);
    assert!(((cam.eye() - focus).length() - (eye0 - focus).length() * 0.9).abs() < 1e-6);
}
