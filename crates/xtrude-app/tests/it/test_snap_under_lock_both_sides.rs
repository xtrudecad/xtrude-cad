// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window tests: under an axis lock every reference is found
//! whichever side of the start it is on, through the real viewport and five
//! cameras; the level foot at the start draws nothing; the tool's key again
//! releases the first point. (The engine half is in xtrude-core.) The
//! parametrized cameras run as a loop inside each test.

use crate::common;
use xtrude_core::Axis;

use common::window;
use glam::DVec3;
use xtrude_app::input::{Button, Modifiers, MouseEvent};
use xtrude_app::tools::line::LineTool;
use xtrude_app::viewport::Viewport;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::SnapKind;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// Short X-parallel dashes at three heights, on BOTH sides of the start.
const DASHES: [(f64, f64, f64); 6] = [
    (-1.5, 0.8, 0.6),
    (1.5, 0.8, 0.6),
    (-1.5, -0.8, 1.4),
    (1.5, -0.8, 1.4),
    (-1.0, 0.0, 0.0),
    (1.0, 0.0, 0.0),
];
const START: DVec3 = DVec3::new(0.3, 0.2, 0.0);
const CAMERAS: [(&str, Option<f64>, Option<f64>); 5] = [
    ("iso", None, None),
    ("obl-a", Some(-30.), Some(12.)),
    ("obl-b", Some(-75.), Some(55.)),
    ("obl-c", Some(20.), Some(25.)),
    ("obl-d", Some(160.), Some(40.)),
];

fn scene() -> MainWindow {
    let mut w = window();
    w.viewport.resize(1600., 900.);
    w.viewport.camera.set_aspect(1600, 900);
    for (x, y, z) in DASHES {
        w.viewport
            .scene
            .mesh_mut()
            .add_edge(v(x, y, z), v(x + 0.4, y, z))
            .expect("dash");
    }
    w.viewport.scene.version += 1;
    w.activate_tool("line");
    w
}

fn camera(vp: &mut Viewport, (name, yaw, pitch): (&str, Option<f64>, Option<f64>)) {
    match (yaw, pitch) {
        (Some(yaw), Some(pitch)) => {
            vp.camera.yaw = yaw.to_radians();
            vp.camera.pitch = pitch.to_radians();
        }
        _ => vp.camera.set_view(name),
    }
    vp.camera.distance = 10.0;
    vp.camera.target = v(0., 0., 0.5);
}

fn hover(vp: &mut Viewport, world: DVec3) -> (f64, f64) {
    let px = vp.world_to_pixel(world).expect("on screen");
    vp.input.last_mouse_pos = Some(px);
    vp.process_hover(px.x, px.y, Modifiers::NONE);
    px.tuple()
}

fn lock(vp: &mut Viewport, axis: Axis) {
    let tool = vp.tool_mut::<LineTool>().expect("the Line tool");
    tool.start_point = Some(START);
    tool.chain_first_point = Some(START);
    vp.inference.axis_lock = Some(axis);
}

#[test]
fn test_every_dash_is_found_under_the_x_lock_on_either_side() {
    for cam in CAMERAS {
        let mut w = scene();
        let vp = &mut w.viewport;
        camera(vp, cam);
        lock(vp, Axis::X);
        for (x, y, z) in DASHES.into_iter().chain([(0.0, 0.0, 0.0)]) {
            hover(vp, v(x, y, z));
            let s = vp.inference.last_snap.clone().expect("a snap");
            let at = (cam.0, x, y, z, s.kind.clone());
            assert!(
                matches!(s.kind, SnapKind::FromPoint | SnapKind::Origin),
                "{at:?}"
            );
            assert!((s.point.x - x).abs() < 1e-5, "{at:?}");
            assert!((s.point.y - START.y).abs() < 1e-5, "{at:?}");
            assert!((s.point.z - START.z).abs() < 1e-5, "{at:?}");
        }
    }
}

#[test]
fn test_under_the_z_lock_the_dashes_at_the_starts_own_level_show_too() {
    for cam in CAMERAS {
        let mut w = scene();
        let vp = &mut w.viewport;
        camera(vp, cam);
        lock(vp, Axis::Z);
        for (x, y, z) in DASHES {
            hover(vp, v(x, y, z));
            let s = vp.inference.last_snap.clone().expect("a snap");
            let at = (cam.0, x, y, z, s.kind.clone());
            assert_eq!(s.kind, SnapKind::FromPoint, "{at:?}");
            assert!((s.point.z - z).abs() < 1e-5, "{at:?}");
            assert!((s.point.x - START.x).abs() < 1e-5, "{at:?}");
            // …and the guide runs from the dash to the foot, so a dash at
            // the start's own height reads as «level with it».
            let g = s.guide.expect("a guide");
            assert!((g.0 - v(x, y, z)).length() < 1e-5, "{at:?}");
        }
    }
}

#[test]
fn test_clicking_the_level_foot_at_the_start_draws_nothing_and_breaks_nothing() {
    let mut w = scene();
    let vp = &mut w.viewport;
    camera(vp, CAMERAS[0]);
    lock(vp, Axis::Z);
    let px = hover(vp, v(-1.0, 0.0, 0.0)); // a dash at the start's level
    let s = vp.inference.last_snap.clone().expect("a snap");
    assert_eq!(s.kind, SnapKind::FromPoint);
    assert!((s.point - START).length() < 1e-6);
    let edges = vp.scene.mesh().edges.len();
    vp.mouse_press(&MouseEvent::button(px.0, px.1, Button::Left, true));
    assert_eq!(vp.scene.mesh().edges.len(), edges); // no zero-length edge
    let start = vp.tool_start_point().expect("still drawing");
    assert!((start - START).length() < 1e-6);
}

// ---- (5) L again resets the Line -------------------------------------------

#[test]
fn test_pressing_the_tools_key_again_releases_the_first_point() {
    let mut w = scene();
    camera(&mut w.viewport, CAMERAS[0]);
    lock(&mut w.viewport, Axis::X);
    assert!(w.viewport.tool_start_point().is_some());
    w.activate_tool("line");
    assert!(w.viewport.tool_start_point().is_none());
    assert!(w.viewport.inference.axis_lock.is_none());
}
