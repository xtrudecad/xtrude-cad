// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A snapped point off the captured plane picks the shape's plane — filling a
//! slot between the midpoints of its two sides lands on the plate's middle
//! plane.

use crate::common;
use xtrude_core::Axis;

use common::{stub, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::tools::arc::{ArcTool, CenterArcTool, ThreePointArcTool};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{Plane, Tool};

const BOTTOM_RIGHT: DVec3 = DVec3::new(4.0, 0.10, 0.90); // midpoint, right side, bottom
const TOP_LEFT: DVec3 = DVec3::new(0.27, 0.10, 3.31); // midpoint, left side, top
const SIDE_PLANE: Plane = (DVec3::new(4.0, 0.0, 0.0), DVec3::new(-1.0, 0.0, 0.0));

fn rect() -> RectangleTool {
    let mut t = RectangleTool::new();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.hover_point = Some(TOP_LEFT);
    t
}

fn on_plate_middle(pts: &[DVec3]) -> bool {
    pts.iter().all(|p| (p.y - 0.10).abs() < 1e-6)
}

#[test]
fn test_the_slot_fills_on_the_plate_middle_plane() {
    let t = rect();
    let (anchor, far) = t.span(t.hover_point.unwrap());
    let (du, dv) = t.dimensions(anchor, far);
    assert_eq!((du.abs() * 100.).round() / 100., 3.73);
    assert_eq!((dv.abs() * 100.).round() / 100., 2.41);
    assert!(on_plate_middle(&t.corners(anchor, far)));
}

#[test]
fn test_a_corner_on_the_plane_keeps_the_captured_plane() {
    let mut t = rect();
    t.hover_point = Some(v3(4.0, 0.15, 2.0));
    assert_eq!(t.drawing_plane(), SIDE_PLANE);
}

#[test]
fn test_a_true_3d_diagonal_keeps_the_captured_plane() {
    let mut t = rect();
    t.hover_point = Some(v3(0.27, 0.0, 3.31));
    assert_eq!(t.drawing_plane(), SIDE_PLANE);
}

#[test]
fn test_an_arrow_lock_never_yields() {
    let mut t = rect();
    t.lock.plane_lock = Some(Axis::X);
    assert_eq!(t.drawing_plane(), SIDE_PLANE);
}

#[test]
fn test_a_circle_turns_to_the_snapped_rim() {
    for mut t in [RadialTool::circle(), RadialTool::polygon()] {
        t.start_point = Some(BOTTOM_RIGHT);
        t.work_plane = Some(SIDE_PLANE);
        t.hover_point = Some(TOP_LEFT);
        let pts = t.points(BOTTOM_RIGHT, TOP_LEFT);
        assert!(on_plate_middle(&pts));
        assert!((pts[0] - TOP_LEFT).length() < 1e-6, "a vertex on the snap");
    }
}

#[test]
fn test_a_circle_labels_the_radius_it_draws() {
    let _m = MetreDocument::new();
    let vp = stub();
    let mut t = RadialTool::circle();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.hover_point = Some(v3(0.27, 0.60, 3.31));
    let pts = t.points(BOTTOM_RIGHT, t.hover_point.unwrap());
    let r = (pts[0] - BOTTOM_RIGHT).length();
    assert_eq!(t.drawing_plane(), SIDE_PLANE);
    assert!(
        t.value_label(&vp).unwrap().0.starts_with("R 2.46"),
        "√(0.5² + 2.41²)"
    );
    assert!((r - 2.461).abs() < 1e-3);
}

#[test]
fn test_a_two_point_arc_ends_on_the_snapped_end() {
    let mut t = ArcTool::new();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.end_point = Some(TOP_LEFT);
    t.adopt_snapped_plane();
    assert!(
        t.work_plane.unwrap().1.y.abs() > 0.999,
        "the bulge is read on the plate"
    );
    let bulge = v3(2.0, 0.10, 3.9); // free cursor, on the new plane
    t.hover_point = Some(bulge);
    let pts = t.points(Some(bulge), None, None);
    assert!(pts.len() > 2 && on_plate_middle(&pts));
    assert!((pts[pts.len() - 1] - TOP_LEFT).length() < 1e-6);
}

#[test]
fn test_a_three_point_arc_takes_the_plane_through_its_points() {
    let mut t = ThreePointArcTool::new();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.mid_point = Some(v3(2.0, 0.10, 3.9));
    t.adopt_snapped_plane();
    t.hover_point = Some(TOP_LEFT);
    let pts = t.points(TOP_LEFT);
    assert!(on_plate_middle(&pts));
    assert!((pts[pts.len() - 1] - TOP_LEFT).length() < 1e-6);
}

#[test]
fn test_a_centre_arc_swings_its_snapped_arm() {
    let _m = MetreDocument::new();
    let vp = stub();
    let mut t = CenterArcTool::new();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.hover_point = Some(TOP_LEFT);
    assert!(t.value_label(&vp).unwrap().0.starts_with("R 4.4"));
    t.arm_point = Some(TOP_LEFT);
    t.adopt_snapped_plane();
    let pts = t.points(90.0);
    assert!(on_plate_middle(&pts));
    assert!((pts[0] - TOP_LEFT).length() < 1e-6);
}

#[test]
fn test_the_arrow_lock_holds_for_every_shape() {
    let mut c = RadialTool::circle();
    c.start_point = Some(BOTTOM_RIGHT);
    c.work_plane = Some(SIDE_PLANE);
    c.lock.plane_lock = Some(Axis::X);
    c.hover_point = Some(TOP_LEFT);
    assert_eq!(c.drawing_plane(), SIDE_PLANE);
    let mut a = ArcTool::new();
    a.start_point = Some(BOTTOM_RIGHT);
    a.work_plane = Some(SIDE_PLANE);
    a.lock.plane_lock = Some(Axis::X);
    a.hover_point = Some(TOP_LEFT);
    assert_eq!(a.drawing_plane(), SIDE_PLANE);
    let mut t = ThreePointArcTool::new();
    t.start_point = Some(BOTTOM_RIGHT);
    t.work_plane = Some(SIDE_PLANE);
    t.lock.plane_lock = Some(Axis::X);
    t.hover_point = Some(TOP_LEFT);
    assert_eq!(t.drawing_plane(), SIDE_PLANE);
    let mut ca = CenterArcTool::new();
    ca.start_point = Some(BOTTOM_RIGHT);
    ca.work_plane = Some(SIDE_PLANE);
    ca.lock.plane_lock = Some(Axis::X);
    ca.hover_point = Some(TOP_LEFT);
    assert_eq!(ca.drawing_plane(), SIDE_PLANE);
}
