// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! At near-horizon views a HORIZONTAL captured plane yields to the vertical
//! plane through the start point (walls are untouched), and near the horizon a
//! grazing ground plane gives no point at all — unless a face is under the
//! cursor.

use crate::common;

use common::{sized_viewport, v3};
use glam::DVec3;
use xtrude_app::tools::line::LineTool;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;

/// A Line tool with the ground captured and a start point at (2, 3, 0).
fn with_tool(vp: &mut Viewport, normal: DVec3) {
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    let t = vp.tool_mut::<LineTool>().unwrap();
    t.work_plane = Some((DVec3::ZERO, normal));
    t.start_point = Some(v3(2., 3., 0.));
}

#[test]
fn test_iso_view_keeps_captured_ground_plane() {
    let mut vp = sized_viewport(800., 600.);
    with_tool(&mut vp, DVec3::Z);
    vp.camera.set_view("iso");
    let (_, n) = vp.current_work_plane(None);
    assert!(n.z.abs() > 0.99);
}

#[test]
fn test_horizon_view_unlocks_vertical_drawing() {
    let mut vp = sized_viewport(800., 600.);
    with_tool(&mut vp, DVec3::Z);
    vp.camera.set_view("front");
    let (pt, n) = vp.current_work_plane(None);
    assert!(n.z.abs() < 1e-6);
    assert!((pt - v3(2., 3., 0.)).length() < 1e-9);
}

#[test]
fn test_captured_wall_plane_is_untouched_at_horizon() {
    let mut vp = sized_viewport(800., 600.);
    with_tool(&mut vp, DVec3::X);
    vp.camera.set_view("front");
    let (_, n) = vp.current_work_plane(None);
    assert!(n.x.abs() > 0.99);
}

/// The ground captured and the camera 16° below the horizontal: the ray
/// grazes the plane and the intersection runs off.
fn grazing_camera(pitch_deg: f64) -> Viewport {
    let mut vp = sized_viewport(1600., 1000.);
    with_tool(&mut vp, DVec3::Z);
    let c = &mut vp.camera;
    c.target = DVec3::ZERO;
    c.distance = 60.0;
    c.yaw = (-90f64).to_radians();
    c.pitch = pitch_deg.to_radians();
    vp
}

#[test]
fn test_near_the_horizon_the_point_does_not_run_away() {
    let mut vp = grazing_camera(16.0);
    let near = vp.world_from_pixel(800., 700.);
    assert!(near.is_some_and(|p| p.length() < 60.0));
    assert_eq!(vp.world_from_pixel(800., 200.), None);
}

#[test]
fn test_a_normal_view_does_not_change() {
    let mut vp = grazing_camera(25.0);
    for py in (200..1000).step_by(50) {
        assert!(vp.world_from_pixel(800., py as f64).is_some(), "{py}");
    }
}

#[test]
fn test_with_a_face_under_the_cursor_the_point_lands_on_it() {
    let mut vp = grazing_camera(16.0);
    // A wall across the view, 30 m from the target, in the model; the ray
    // passes z ≈ 13.4 there, so it is made 20 m tall.
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(-10., 30., 0.),
            v3(10., 30., 0.),
            v3(10., 30., 20.),
            v3(-10., 30., 20.),
        ],
        &[],
    );
    *vp.scene.mesh_mut() = m;
    vp.scene.version += 1;
    let p = vp
        .world_from_pixel(800., 200.)
        .expect("there was a face right there");
    assert!((p.y - 30.0).abs() < 1e-3, "{p}");
    assert!(p.length() < 60.0);
}
