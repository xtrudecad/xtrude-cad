// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! With the camera near the ground the pull follows the cursor past the
//! horizon, and a click on the top of the box being pulled commits it; the
//! drag plane holds the push axis and faces the camera.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{click, stub, v3, window};
use std::f64::consts::FRAC_PI_2;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_core::math::VecExt;
use xtrude_model::ids::MeshId;

fn hover(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    vp.process_hover(x, y, Modifiers::NONE);
}

fn click_at(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    click(vp, x, y, Modifiers::NONE);
}

fn tool(vp: &Viewport) -> &PushPullTool {
    vp.tool::<PushPullTool>().unwrap()
}

#[test]
fn test_the_pull_follows_the_cursor_past_the_horizon_and_the_top_click_commits() {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1400., 800.);
    vp.camera.set_aspect(1400, 800);
    let c = &mut vp.camera;
    c.target = v3(4., 2.5, 0.);
    c.distance = 18.0;
    c.pitch = 6f64.to_radians(); // 6° above the ground
    c.yaw = (-60f64).to_radians();
    let face = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(8., 0., 0.),
            v3(8., 5., 0.),
            v3(0., 5., 0.),
        ],
        &[],
    );
    vp.scene.version += 1;
    w.activate_tool("pushpull");
    let vp = &mut w.viewport;
    let ScreenPt { x: cx, y: cy } = vp.world_to_pixel(v3(4., 2.5, 0.)).unwrap();
    // The centre ray meets the ground at exactly the 6° grazing limit
    // (a hair either way under rounding); 10 px lower is steeper.
    let cy = cy + 10.0;
    hover(vp, cx, cy);
    assert_eq!(tool(vp).hovered_face, Some((MeshId::LOOSE, face)));
    click_at(vp, cx, cy);
    assert!(tool(vp).dragging());
    // The cursor climbs past the horizon; the extrusion keeps growing.
    let mut heights = Vec::new();
    for dy in [40., 120., 200., 280., 360., 400.] {
        hover(vp, cx, cy - dy);
        heights.push(tool(vp).pp.extrusion);
    }
    let mut sorted = heights.clone();
    sorted.sort_by(f64::total_cmp);
    assert!(heights == sorted && heights[0] > 0.3, "{heights:?}");
    assert!(heights[5] > 5.0, "{heights:?}");
    // Click ON THE TOP of the box being pulled.
    let h = tool(vp).pp.extrusion;
    let ScreenPt { x: px, y: py } = vp.world_to_pixel(v3(4., 2.5, h)).unwrap();
    assert!(py < cy - 300.0); // well above the horizon
    hover(vp, px, py);
    click_at(vp, px, py);
    assert!(!tool(vp).dragging());
    let m = vp.scene.mesh();
    assert_eq!(m.face_ids().len(), 6); // the box is committed
    let top = m
        .verts
        .iter()
        .map(|(_, v)| v.position.z)
        .fold(f64::MIN, f64::max);
    assert!(top > 5.0);
}

#[test]
fn test_drag_plane_contains_the_push_axis_and_faces_the_camera() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 1., 0.),
            v3(2., 1., 0.),
            v3(2., 3., 0.),
            v3(0., 3., 0.),
        ],
        &[],
    );
    let mut t = PushPullTool::new();
    assert!(t.drag_plane(&vp).is_none()); // idle: the viewport decides
    t.begin(&mut vp, MeshId::LOOSE, f); // anchor (1, 2, 0), normal +Z
    let (anchor, n) = (t.pp.anchor.unwrap(), t.pp.normal.unwrap());
    vp.camera.target = glam::DVec3::ZERO;
    vp.camera.distance = 12.0;
    vp.camera.yaw = 0.3;
    vp.camera.pitch = 0.25; // the eye a little above the ground
    let (point, normal) = t.drag_plane(&vp).expect("a plane");
    assert_eq!(point, anchor);
    assert!(normal.dot(n).abs() < 1e-9);
    let forward = (vp.camera.target - vp.camera.eye()).unit_or_zero();
    assert!(normal.dot(forward) > 0.9);
    // Looking straight along the axis there is no such plane: fall back.
    vp.camera.target = anchor;
    vp.camera.pitch = FRAC_PI_2;
    assert!(t.drag_plane(&vp).is_none());
}
