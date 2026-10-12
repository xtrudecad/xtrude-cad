// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Which tools ask for the on-screen axis inference (the snap tests live in
//! crates/xtrude-core).

use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::Tool;

/// Line, Move and the Measure tool. The Protractor does not (its
/// arms live in the disc's plane), nor the planar Rectangle.
#[test]
fn test_the_tools_that_ask_for_it() {
    assert_eq!(LineTool::new().snap_prefs().screen_axis_px, Some(9.0));
    assert_eq!(MoveTool::new().snap_prefs().screen_axis_px, Some(9.0));
    assert_eq!(MeasureTool::new().snap_prefs().screen_axis_px, Some(9.0));
    assert_eq!(ProtractorTool::new().snap_prefs().screen_axis_px, None);
    assert_eq!(RectangleTool::new().snap_prefs().screen_axis_px, None);
}
