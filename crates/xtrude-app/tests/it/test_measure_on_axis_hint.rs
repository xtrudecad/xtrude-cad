// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Measure tool says it is on an axis before its first click.

use crate::common;
use xtrude_app::input::ScreenPt;
use xtrude_core::Axis;

use common::sized_viewport;
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_core::snap::SnapKind;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

#[test]
fn test_the_measure_tool_shows_on_axis_before_its_first_click() {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("iso");
    vp.camera.target = v(4., 2.);
    vp.camera.distance = 16.0;
    vp.set_active_tool(Some(Box::new(MeasureTool::default())));
    let ScreenPt { x, y } = vp.world_to_pixel(v(3., 0.)).unwrap();
    vp.process_hover(x, y + 2., Modifiers::NONE);
    let s = vp.inference.last_snap.clone().expect("a snap");
    assert!(
        s.kind == SnapKind::OnAxis && s.axis == Some(Axis::X),
        "{:?}",
        s.kind
    );
    assert!(s.point.y.abs() < 1e-6 && s.point.z.abs() < 1e-6);
    vp.set_active_tool(Some(Box::new(LineTool::new()))); // not a source for Line
    vp.process_hover(x, y + 2., Modifiers::NONE);
    assert_ne!(
        vp.inference.last_snap.map(|s| s.kind),
        Some(SnapKind::OnAxis)
    );
}
