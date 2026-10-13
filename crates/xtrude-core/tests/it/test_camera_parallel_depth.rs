// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The parallel projection's depth range is fitted to the scene
//! (`OrbitCamera::depth_box`) instead of ±`zfar`: the `f32` depth buffer
//! spread over ±10 km told apart only depths about a millimetre apart, and
//! edges behind a thin plate showed through it.

use xtrude_core::camera::{standard_view, OrbitCamera};
use xtrude_core::math::MatExt;
use xtrude_core::DVec3;

fn depth(cam: &OrbitCamera, p: DVec3) -> f64 {
    (p - cam.eye()).dot(cam.forward())
}

fn corners(lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let mut out = Vec::new();
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                out.push(DVec3::new(x, y, z));
            }
        }
    }
    out
}

/// A part about half a metre across, framed in parallel.
fn framed_part() -> (OrbitCamera, (DVec3, DVec3)) {
    let b = (DVec3::new(-0.1, 0.05, 0.0), DVec3::new(0.4, 0.35, 0.2));
    let mut cam = OrbitCamera {
        perspective: false,
        aspect: 1400.0 / 900.0,
        depth_box: Some(b),
        ..OrbitCamera::new()
    };
    cam.fit_box(b.0, b.1, 1.1);
    (cam, b)
}

/// Every corner of the box lies strictly inside the clip volume's depth.
fn assert_box_between_planes(cam: &OrbitCamera, b: (DVec3, DVec3), what: &str) {
    let mvp = cam.projection_matrix() * cam.view_matrix();
    for c in corners(b.0, b.1) {
        let z = mvp.project_point(c).z;
        assert!(z > -1.0 && z < 1.0, "{what}: corner {c} at clip z {z}");
    }
}

#[test]
fn test_the_range_is_fitted_to_the_part_not_to_ten_kilometres() {
    let (cam, _) = framed_part();
    let (near, far) = cam.depth_range();
    assert!(far - near < 100.0, "range {near}..{far}");
    let unfitted = OrbitCamera {
        depth_box: None,
        ..cam.clone()
    };
    assert_eq!(unfitted.depth_range(), (-cam.zfar, cam.zfar));
}

#[test]
fn test_the_box_is_between_the_planes_from_every_standard_view() {
    let (mut cam, b) = framed_part();
    for name in ["top", "bottom", "front", "back", "right", "left", "iso"] {
        let (yaw, pitch) = standard_view(name).expect("a standard view");
        cam.yaw = yaw;
        cam.pitch = pitch;
        cam.fit_box(b.0, b.1, 1.1);
        assert_box_between_planes(&cam, b, name);
    }
}

#[test]
fn test_the_box_is_between_the_planes_with_the_eye_inside_it() {
    let (mut cam, b) = framed_part();
    // Zoomed right in on a corner: the eye is inside the box, half of it
    // behind the eye, and it must all still show.
    cam.target = b.0 + DVec3::splat(0.01);
    cam.distance = 0.03;
    // From the iso yaw (−45°) round to +45°: the eye up and toward +X, +Y.
    cam.orbit(-450.0, 0.0, 900);
    let eye = cam.eye();
    assert!(eye.cmpgt(b.0).all() && eye.cmplt(b.1).all(), "eye {eye}");
    assert_box_between_planes(&cam, b, "eye inside");
    assert!(cam.depth_range().0 < 0.0);
}

#[test]
fn test_what_is_around_the_target_stays_between_the_planes() {
    // A small part, the camera panned away from it: the guides, the axes
    // and a rubber band drawn around the view are still in range.
    let b = (DVec3::ZERO, DVec3::splat(0.01));
    let cam = OrbitCamera {
        perspective: false,
        target: DVec3::new(2.0, 0.0, 0.0),
        distance: 0.5,
        depth_box: Some(b),
        ..OrbitCamera::new()
    };
    let (near, far) = cam.depth_range();
    let at_target = depth(&cam, cam.target);
    assert!(near < at_target - 15.0 * cam.distance);
    assert!(far > at_target + 15.0 * cam.distance);
    assert_box_between_planes(&cam, b, "panned away");
}

#[test]
fn test_clip_to_depth_cuts_at_the_fitted_planes() {
    let (cam, _) = framed_part();
    let (near, far) = cam.depth_range();
    let f = cam.forward();
    let (a, b) = cam
        .clip_to_depth(cam.target - f * 1.0e5, cam.target + f * 1.0e5)
        .expect("in view");
    assert!((depth(&cam, a) - near).abs() < 1e-6, "{}", depth(&cam, a));
    assert!((depth(&cam, b) - far).abs() < 1e-6, "{}", depth(&cam, b));
    // Past the far plane: nothing.
    let beyond = cam.target + f * (far + 1.0);
    assert!(cam.clip_to_depth(beyond, beyond + f).is_none());
}

#[test]
fn test_perspective_ignores_the_depth_box() {
    let (cam, _) = framed_part();
    let persp = OrbitCamera {
        perspective: true,
        ..cam.clone()
    };
    let plain = OrbitCamera {
        depth_box: None,
        ..persp.clone()
    };
    assert_eq!(persp.projection_matrix(), plain.projection_matrix());
    assert_eq!(persp.depth_range(), (persp.near(), persp.zfar));
    let f = persp.forward();
    let (a, z) = (cam.target - f * 1.0e5, cam.target + f * 1.0e5);
    assert_eq!(persp.clip_to_depth(a, z), plain.clip_to_depth(a, z));
}
