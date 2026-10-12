// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The drawn axes at every zoom: the solid + line reaches the GPU already
//! cut to what the camera sees in depth. Sent whole (a hundred kilometres
//! long), the GPU clipped it against the near plane in `f32`, and close up,
//! where that plane is millimetres from the eye, the line came out flung
//! across the screen, drawn as blocks, or missing, depending on the zoom.

use glam::DVec3;
use xtrude_app::viewport::render::push_axis;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::Axis;
use xtrude_render::frame::FrameData;

fn at(f: &FrameData, i: usize) -> DVec3 {
    let q = f.lines[i].pos;
    DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)
}

#[test]
fn test_the_solid_axis_line_lies_between_the_near_and_far_planes_at_every_zoom() {
    let mut cam = OrbitCamera::new();
    cam.target = DVec3::new(0.02, 0.01, 0.0);
    // Looking down (+Z runs behind the eye) and toward −X, −Y (+X and +Y
    // run toward the viewer), as in the reports.
    for yaw_deg in [-45.0f64, -135.0, 135.0, 45.0] {
        cam.yaw = yaw_deg.to_radians();
        let mut distance = 0.02;
        while distance < 50.0 {
            cam.distance = distance;
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                let mut f = FrameData::default();
                push_axis(&mut f, &cam, axis, distance * 0.008, [1.0; 4]);
                for i in 0..2 {
                    let d = (at(&f, i) - cam.eye()).dot(cam.forward());
                    assert!(
                        d >= cam.near() && d <= cam.zfar * 1.000_001,
                        "{axis:?} at {distance} m, yaw {yaw_deg}: depth {d}"
                    );
                }
            }
            distance *= 1.25;
        }
    }
}

#[test]
fn test_the_solid_axis_line_still_starts_at_the_origin_and_runs_along_the_axis() {
    let cam = OrbitCamera {
        distance: 0.1,
        ..OrbitCamera::new()
    };
    let mut f = FrameData::default();
    push_axis(&mut f, &cam, Axis::Z, 0.001, [1.0; 4]);
    let (o, tip) = (at(&f, 0), at(&f, 1));
    assert!(o.length() < 1e-9);
    assert!(tip.z > 0.0 && tip.x.abs() < 1e-6 && tip.y.abs() < 1e-6);
}
