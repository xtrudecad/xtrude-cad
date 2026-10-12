// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The world origin is a point inference and beats "from point" and the axis
//! line.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{sized_viewport, v3};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::ToolContext;
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::edits::build_add_edges;

fn vp() -> Viewport {
    sized_viewport(1400., 900.)
}

fn plan_view(vp: &mut Viewport, distance: f64) {
    let c = &mut vp.camera;
    c.target = v3(0.1, -0.05, 0.);
    c.distance = distance;
    c.yaw = (-90f64).to_radians();
    c.pitch = 89f64.to_radians();
}

fn hover(vp: &mut Viewport, world: DVec3, off: (f64, f64)) -> SnapResult {
    let ScreenPt { x, y } = vp.world_to_pixel(world).unwrap();
    vp.process_hover(x + off.0, y + off.1, Modifiers::NONE);
    vp.inference.last_snap.clone().expect("a snap")
}

fn add_edge(vp: &mut Viewport, a: DVec3, b: DVec3) {
    let cmd = build_add_edges(&vp.scene, &[(a, b)], true, vec![], None);
    vp.execute_boxed(cmd);
}

#[test]
fn test_the_origin_wins_over_a_from_point_on_the_axis() {
    let mut vp = vp();
    // A stub on the red axis; its end ENCOURAGED, then the cursor comes to
    // the origin — aligned with it.
    add_edge(&mut vp, v3(0.8, 0., 0.), v3(1.0, 0., 0.));
    plan_view(&mut vp, 0.7); // zoomed right in
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    vp.inference.encouraged = vec![v3(0.8, 0., 0.)];
    for off in [(0., 0.), (3., 0.), (-4., 1.), (5., -3.)] {
        let s = hover(&mut vp, DVec3::ZERO, off);
        assert_eq!(s.kind, SnapKind::Origin, "{off:?} {}", s.point);
        assert_eq!(s.point, DVec3::ZERO);
    }
}

#[test]
fn test_the_origin_wins_over_the_axis_line_from_a_start_point() {
    let mut vp = vp();
    plan_view(&mut vp, 0.7);
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    let start = v3(0.8, 0., 0.);
    let ScreenPt { x, y } = vp.world_to_pixel(start).unwrap();
    vp.process_hover(x, y, Modifiers::NONE);
    let mut ctx = ToolContext::at(start);
    ctx.screen = (x, y);
    ctx.snap = vp.inference.last_snap.clone();
    vp.with_tool(|t, vp| t.on_click(vp, &ctx));
    for off in [(0., 0.), (4., 0.), (-5., 2.)] {
        let s = hover(&mut vp, DVec3::ZERO, off);
        assert_eq!(s.kind, SnapKind::Origin, "{off:?} {}", s.point);
    }
    // Away from it, the axis inference still runs the line along X.
    let s = hover(&mut vp, v3(0.3, 0., 0.), (0., 2.));
    assert!(
        matches!(s.kind, SnapKind::Axis | SnapKind::AxisInference),
        "{:?}",
        s.kind
    );
    vp.with_tool(|t, vp| t.on_cancel(vp));
}

#[test]
fn test_a_corner_on_the_origin_still_reads_as_the_origin() {
    let mut vp = vp();
    add_edge(&mut vp, DVec3::ZERO, v3(1.0, 0., 0.));
    plan_view(&mut vp, 3.0);
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    let s = hover(&mut vp, DVec3::ZERO, (2., 1.));
    assert!(
        s.kind == SnapKind::Origin && s.point == DVec3::ZERO,
        "{:?}",
        s.kind
    );
}
