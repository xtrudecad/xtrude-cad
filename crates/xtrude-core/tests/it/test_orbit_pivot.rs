// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Orbit pivot: the camera half (the viewport pivot-picking tests live with
//! the app).

use xtrude_core::camera::OrbitCamera;
use xtrude_core::DVec3;

#[test]
fn test_orbit_about_keeps_distance_level_horizon_and_the_pivot() {
    let mut cam = OrbitCamera::new();
    cam.target = DVec3::new(10., 0., 0.);
    cam.distance = 30.0;
    let pivot = DVec3::new(12., 3., 1.);
    let rel_before = (pivot - cam.eye()).length();
    cam.orbit_about(pivot, 50.0, -30.0, 600);
    assert!((cam.distance - 30.0).abs() < 1e-9);
    // in f64 the rigid turn holds far tighter than a float32 1e-4
    assert!(((pivot - cam.eye()).length() - rel_before).abs() < 1e-9);
    for _ in 0..40 {
        cam.orbit_about(pivot, 0.0, 400.0, 600);
    }
    assert!(cam.pitch <= 89f64.to_radians() + 1e-9);
}
