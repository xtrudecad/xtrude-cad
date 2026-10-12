// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Zoom-to-cursor keeps the world point under the pointer fixed on screen.

use xtrude_core::camera::{OrbitCamera, MIN_DISTANCE};
use xtrude_core::math::MatExt;
use xtrude_core::DMat4;
use xtrude_core::DVec3;

#[test]
fn test_zoom_to_keeps_focus_under_cursor() {
    let mut cam = OrbitCamera::new();
    let focus = DVec3::new(3., 2., 0.);
    let eye0 = cam.eye();
    let dist0 = cam.distance;
    cam.zoom_to(1.0, focus, 0.0);
    let factor = cam.distance / dist0;
    assert!(factor < 1.0);
    let v0 = eye0 - focus;
    let v1 = cam.eye() - focus;
    assert!((v1.length() / v0.length() - factor).abs() < 1e-5);
    assert!(v0.normalize().dot(v1.normalize()) > 0.99999);
}

#[test]
fn test_zoom_to_respects_distance_clamp() {
    let mut cam = OrbitCamera::new();
    cam.distance = MIN_DISTANCE * 1.2;
    cam.zoom_to(5.0, DVec3::ZERO, 0.0);
    assert!(cam.distance >= MIN_DISTANCE);
}

#[test]
fn test_you_can_get_close_enough_to_look_at_a_component() {
    let mut cam = OrbitCamera::new();
    cam.distance = 2.0;
    for _ in 0..200 {
        cam.zoom(1.0);
    }
    assert!(cam.distance <= 0.1, "the wheel still hits a wall");
    assert!(cam.distance >= MIN_DISTANCE);
}

#[test]
fn test_the_near_plane_gets_out_of_the_way_when_you_come_close() {
    let mut cam = OrbitCamera::new();
    cam.distance = 20.0;
    let unchanged = DMat4::perspective_rh_gl(cam.fov_deg.to_radians(), cam.aspect, 0.1, cam.zfar);
    let got = cam.projection_matrix();
    assert!(
        got.fuzzy_eq(&unchanged, 1e-9),
        "the near plane moved at a normal working distance"
    );
    cam.distance = 0.05;
    let close = cam.projection_matrix();
    let p = cam.eye() + (cam.target - cam.eye()).normalize() * 0.02;
    let clip = (close * cam.view_matrix()).project_point(p);
    assert!(clip.z > -1.0);
}
