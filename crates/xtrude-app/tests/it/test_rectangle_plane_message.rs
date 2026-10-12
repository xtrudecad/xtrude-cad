// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! When a rectangle comes out flat because its far corner is off the drawing
//! plane, the message says so and names the arrow that fixes it; a snapped far
//! corner picks the plane that holds both corners.

use glam::DVec3;
use xtrude_app::tools::rectangle::RectangleTool;

/// The corners of a wall, to the millimetre.
const CORNER: DVec3 = DVec3::new(3.309, 2.777, 0.0);
const OPPOSITE: DVec3 = DVec3::new(7.309, 2.927, 0.0);

fn tool(normal: DVec3) -> RectangleTool {
    let mut t = RectangleTool::new();
    t.start_point = Some(CORNER);
    t.work_plane = Some((CORNER, normal));
    t
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

#[test]
fn test_a_corner_off_the_plane_blames_the_plane() {
    let msg = tool(-DVec3::Y).degenerate_reason(OPPOSITE);
    assert!(msg.contains("off the drawing plane"), "{msg}");
    assert!(!msg.contains("opposite corner"), "that was the lie");
}

#[test]
fn test_and_names_the_arrow_that_fixes_it() {
    let msg = tool(-DVec3::Y).degenerate_reason(OPPOSITE);
    assert!(msg.contains("XY") && msg.contains("the Up arrow"), "{msg}");
}

#[test]
fn test_a_genuinely_flat_pick_keeps_the_old_message() {
    let t = tool(DVec3::Z); // horizontal plane
    let msg = t.degenerate_reason(DVec3::new(7.309, 2.777, 0.0)); // same row
    assert!(msg.contains("opposite corner"), "{msg}");
}

#[test]
fn test_the_horizontal_plane_draws_the_step() {
    let t = tool(DVec3::Z);
    let (far, _) = t.square_corner(CORNER, OPPOSITE, false);
    let (du, dv) = t.dimensions(CORNER, far);
    assert!(du.abs() > 1e-6 && dv.abs() > 1e-6);
    assert_eq!(r3(du.abs()), 4.0);
    assert_eq!(r3(dv.abs()), 0.15);
}

#[test]
fn test_a_snapped_corner_now_draws_the_step() {
    let mut t = tool(-DVec3::Y);
    t.hover_point = Some(OPPOSITE);
    let (far, _) = t.square_corner(CORNER, OPPOSITE, false);
    let (du, dv) = t.dimensions(CORNER, far);
    let mut got = [r3(du.abs()), r3(dv.abs())];
    got.sort_by(f64::total_cmp);
    assert_eq!(got, [0.15, 4.0]);
}
