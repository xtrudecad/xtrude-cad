// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `OrbitCamera::clip_to_depth` keeps the part of a segment between the
//! near and far planes, cut in `f64`, so the GPU never clips a long line
//! against a tiny near plane in `f32`.

use xtrude_core::camera::OrbitCamera;
use xtrude_core::DVec3;

fn depth(cam: &OrbitCamera, p: DVec3) -> f64 {
    (p - cam.eye()).dot(cam.forward())
}

/// Close up and looking down: the near plane is a few millimetres.
fn close_up() -> OrbitCamera {
    OrbitCamera {
        distance: 0.15,
        ..OrbitCamera::new()
    }
}

#[test]
fn test_a_segment_through_the_eyes_plane_stops_in_front_of_the_near_plane() {
    let cam = close_up();
    let (a, b) = cam
        .clip_to_depth(DVec3::ZERO, DVec3::Z * 1.0e5)
        .expect("the origin is in view");
    assert_eq!(a, DVec3::ZERO);
    let near = cam.near();
    assert!(near < 0.01);
    assert!(depth(&cam, b) > near);
    assert!(depth(&cam, b) < near * 1.1);
    // Still on the axis.
    assert!(b.x.abs() < 1e-12 && b.y.abs() < 1e-12 && b.z > 0.0);
}

#[test]
fn test_a_segment_past_the_far_plane_stops_at_it() {
    let cam = OrbitCamera::new();
    let far = cam.target + cam.forward() * 1.0e5;
    let (_, b) = cam.clip_to_depth(cam.target, far).expect("in view");
    assert!((depth(&cam, b) - cam.zfar).abs() < 1e-6);
}

#[test]
fn test_a_segment_wholly_behind_the_eye_is_none() {
    let cam = OrbitCamera::new();
    let back = cam.eye() - cam.forward();
    assert!(cam
        .clip_to_depth(back, back - cam.forward() * 10.0)
        .is_none());
    // Parallel to the image plane, behind the eye.
    let side = back + DVec3::Z.cross(cam.forward()).normalize();
    assert!(cam.clip_to_depth(back, side).is_none());
}

#[test]
fn test_a_segment_in_view_is_unchanged() {
    for perspective in [true, false] {
        let cam = OrbitCamera {
            perspective,
            ..OrbitCamera::new()
        };
        let (a, b) = (DVec3::new(-1., 0., 0.), DVec3::new(2., 3., 1.));
        assert_eq!(cam.clip_to_depth(a, b), Some((a, b)));
    }
}

#[test]
fn test_parallel_projection_keeps_what_is_behind_the_eye() {
    // Its near plane is -zfar: things behind the eye still show.
    let cam = OrbitCamera {
        perspective: false,
        ..OrbitCamera::new()
    };
    let (a, b) = cam
        .clip_to_depth(DVec3::ZERO, DVec3::Z * 1.0e5)
        .expect("in view");
    assert_eq!(a, DVec3::ZERO);
    assert!(depth(&cam, b) >= -cam.zfar - 1e-6);
    assert!(b.z > cam.eye().z);
}
