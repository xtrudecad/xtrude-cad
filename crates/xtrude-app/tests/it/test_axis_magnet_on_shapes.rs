// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The drawing tools carry the Line tool's axis magnet, switched off for the
//! clicks that are not a direction from the start point.

use crate::common;
use xtrude_core::Axis;

use common::v3;
use glam::DVec3;
use xtrude_app::tools::arc::{ArcTool, CenterArcTool, ThreePointArcTool};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::dimension::DimensionTool;
use xtrude_app::tools::freehand::FreehandTool;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::rotated_rectangle::RotatedRectangleTool;
use xtrude_app::tools::text::TextTool;
use xtrude_app::tools::Tool;
use xtrude_core::snap::{compute_snap, SnapKind, SnapQuery};

fn magnet(t: &dyn Tool) -> (Option<f64>, Option<f64>) {
    (
        t.snap_prefs().magnetic_axis_deg,
        t.snap_prefs().screen_axis_px,
    )
}

#[test]
fn test_the_shapes_declare_the_line_tools_magnet() {
    let line = magnet(&LineTool::new());
    assert_eq!(line, (Some(3.0), Some(9.0)));
    let tools: Vec<(&str, Box<dyn Tool>)> = vec![
        ("ArcTool", Box::new(ArcTool::new())),
        ("ThreePointArcTool", Box::new(ThreePointArcTool::new())),
        ("CenterArcTool", Box::new(CenterArcTool::new())),
        ("CircleTool", Box::new(RadialTool::circle())),
        ("PolygonTool", Box::new(RadialTool::polygon())),
        (
            "RotatedRectangleTool",
            Box::new(RotatedRectangleTool::new()),
        ),
        ("TextTool", Box::new(TextTool::new())),
        ("DimensionTool", Box::new(DimensionTool::new())),
    ];
    for (name, t) in tools {
        assert_eq!(magnet(t.as_ref()), line, "{name}");
    }
}

#[test]
fn test_freehand_and_rectangle_stay_out() {
    assert_eq!(magnet(&FreehandTool::new()), (None, None));
    assert_eq!(magnet(&RectangleTool::new()), (None, None));
}

#[test]
fn test_the_magnet_is_off_for_clicks_that_are_not_a_direction_from_the_start() {
    let mut arc = ArcTool::new();
    arc.start_point = Some(v3(0., 0., 0.));
    assert_eq!(magnet(&arc), (Some(3.0), Some(9.0))); // the chord end
    arc.end_point = Some(v3(2., 0., 0.));
    assert_eq!(magnet(&arc), (None, None)); // the bulge
    let mut rr = RotatedRectangleTool::new();
    rr.start_point = Some(v3(0., 0., 0.));
    assert_eq!(magnet(&rr), (Some(3.0), Some(9.0))); // the base edge
    rr.base_point = Some(v3(2., 0., 0.));
    assert_eq!(magnet(&rr), (None, None)); // the height
    let mut dim = DimensionTool::new();
    dim.a = Some(v3(0., 0., 0.));
    assert_eq!(magnet(&dim), (Some(3.0), Some(9.0))); // the measured span
    dim.b = Some(v3(2., 0., 0.));
    assert_eq!(magnet(&dim), (None, None)); // the placement
}

fn w2p(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, p.y * 100.0 - p.z * 100.0))
}

#[test]
fn test_a_circles_radius_point_lands_on_the_axis_through_the_engine() {
    let mut tool = RadialTool::circle();
    tool.start_point = Some(v3(0., 0., 0.));
    let start = tool.start_point.unwrap();
    let cursor = v3(2.0, 0.07, 0.); // 2° above +X
    let project = move |s: DVec3, d: DVec3| s + d * (cursor - s).dot(d);
    let mut q = SnapQuery::new(cursor, w2p(cursor).unwrap(), &w2p, 9.0);
    q.edge_threshold_px = Some(14.0);
    q.start_point = Some(start);
    q.project_onto_line = Some(&project);
    q.magnetic_axis_deg = tool.snap_prefs().magnetic_axis_deg;
    q.screen_axis_px = tool.snap_prefs().screen_axis_px;
    let snap = compute_snap(&q, &[]);
    assert!(snap.kind == SnapKind::Axis && snap.axis == Some(Axis::X));
    assert!(snap.point.y.abs() < 1e-6 && (snap.point.x - 2.0).abs() < 1e-6);
}
