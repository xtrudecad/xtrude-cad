// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Zoom Extents frames the model as the camera looks at it NOW.

use xtrude_core::camera::{OrbitCamera, MAX_DISTANCE};
use xtrude_core::{DVec3, DVec4};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// (half-span of the box on screen, every corner inside the window).
fn screen(cam: &OrbitCamera, lo: DVec3, hi: DVec3) -> (f64, bool) {
    let mvp = cam.projection_matrix() * cam.view_matrix();
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                let p = mvp * DVec4::new(x, y, z, 1.0);
                xs.push(p.x / p.w);
                ys.push(p.y / p.w);
            }
        }
    }
    let mm = |v: &[f64]| {
        v.iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)))
    };
    let ((x0, x1), (y0, y1)) = (mm(&xs), mm(&ys));
    let span = (x1 - x0).max(y1 - y0) / 2.0;
    let inside = xs.iter().chain(ys.iter()).all(|v| v.abs() <= 1.0 + 1e-4);
    (span, inside)
}

fn boxes() -> Vec<(&'static str, DVec3, DVec3)> {
    vec![
        ("plaza", v(0., 0., 0.), v(30., 20., 0.2)),
        ("street", v(0., 0., 0.), v(1000., 5., 0.)),
        ("tower", v(0., 0., 0.), v(5., 5., 40.)),
        ("wall", v(0., 0., 0.), v(20., 0.2, 3.)),
    ]
}

#[test]
fn test_the_model_fills_the_window_in_any_view() {
    for (name, lo, hi) in boxes() {
        for view in ["top", "right", "front", "iso"] {
            for perspective in [true, false] {
                for aspect in [1.8, 0.6] {
                    let mut cam = OrbitCamera::new();
                    cam.aspect = aspect;
                    cam.perspective = perspective;
                    cam.set_view(view);
                    cam.fit_box(lo, hi, 1.1);
                    let (span, inside) = screen(&cam, lo, hi);
                    let case = format!("{name} {view} persp={perspective} aspect={aspect}");
                    assert!(inside, "{case}");
                    assert!((span - 1.0 / 1.1).abs() <= 0.02, "{case}: span {span}");
                }
            }
        }
    }
}

#[test]
fn test_a_second_zoom_extents_after_an_orbit_is_not_a_no_op() {
    let (lo, hi) = (v(0., 0., 0.), v(30., 20., 0.2));
    let mut cam = OrbitCamera::new();
    cam.aspect = 1.8;
    cam.fit_box(lo, hi, 1.1);
    let before = (cam.target, cam.distance);
    cam.yaw = (-95f64).to_radians();
    cam.pitch = (-30f64).to_radians();
    cam.fit_box(lo, hi, 1.1);
    assert_ne!((cam.target, cam.distance), before);
    assert!(screen(&cam, lo, hi).1);
}

#[test]
fn test_a_point_and_a_huge_model_stay_in_range() {
    let mut cam = OrbitCamera::new();
    cam.fit_box(v(1., 1., 1.), v(1., 1., 1.), 1.1);
    assert!(cam.distance > 0.0 && cam.distance.is_finite());
    cam.fit_box(v(0., 0., 0.), v(60000., 60000., 10.), 1.1);
    assert!(cam.distance <= MAX_DISTANCE);
}
