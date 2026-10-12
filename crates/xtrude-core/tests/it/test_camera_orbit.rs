// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Orbit and pan agree: both grab the model.

use xtrude_core::camera::OrbitCamera;
use xtrude_core::math::MatExt;
use xtrude_core::DVec3;

const MARK: DVec3 = DVec3::new(0.0, 0.0, 1.0);
const DRAG: f64 = 40.0;
const VIEWPORT_H: i32 = 600;

fn screen_y(cam: &OrbitCamera) -> f64 {
    (cam.projection_matrix() * cam.view_matrix())
        .project_point(MARK)
        .y
}

fn camera() -> OrbitCamera {
    let mut cam = OrbitCamera::new();
    cam.aspect = 4.0 / 3.0;
    cam
}

#[test]
fn test_drag_down_moves_the_model_down_under_both_gestures() {
    let (mut orbited, mut panned) = (camera(), camera());
    let before = screen_y(&orbited);
    assert_eq!(before, screen_y(&panned));
    orbited.orbit(0.0, DRAG, VIEWPORT_H);
    panned.pan(0.0, DRAG, VIEWPORT_H, None);
    assert!(screen_y(&orbited) < before, "orbit sent the model UP");
    assert!(
        screen_y(&panned) < before,
        "pan sent the model UP on a down-drag"
    );
}

#[test]
fn test_drag_down_lifts_the_camera_over_the_model() {
    let mut cam = camera();
    let eye_before = cam.eye().z;
    cam.orbit(0.0, DRAG, VIEWPORT_H);
    assert!(cam.eye().z > eye_before);
}

#[test]
fn test_drag_up_is_the_exact_opposite() {
    let mut cam = camera();
    let before = screen_y(&cam);
    cam.orbit(0.0, -DRAG, VIEWPORT_H);
    assert!(screen_y(&cam) > before);
}

#[test]
fn test_horizontal_axis_was_never_the_broken_one() {
    let screen_x = |cam: &OrbitCamera| {
        (cam.projection_matrix() * cam.view_matrix())
            .project_point(DVec3::X)
            .x
    };
    let (mut orbited, mut panned) = (camera(), camera());
    let before = screen_x(&orbited);
    orbited.orbit(DRAG, 0.0, VIEWPORT_H);
    panned.pan(DRAG, 0.0, VIEWPORT_H, None);
    assert!(screen_x(&orbited) > before);
    assert!(screen_x(&panned) > before);
}

#[test]
fn test_the_poles_are_still_out_of_reach() {
    let mut cam = camera();
    for _ in 0..50 {
        cam.orbit(0.0, 200.0, VIEWPORT_H);
    }
    assert!(cam.pitch <= 89f64.to_radians());
    let mut cam = camera();
    for _ in 0..50 {
        cam.orbit(0.0, -200.0, VIEWPORT_H);
    }
    assert!(cam.pitch >= (-89f64).to_radians());
}
