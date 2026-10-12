// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The arrow-key axis lock lasts one operation, not until Esc. The
//! Down-arrow reference is left alone: it is bound to an edge the user picked.

use crate::common;
use xtrude_core::Axis;

use common::{click, window};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::line::LineTool;
use xtrude_app::viewport::Viewport;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::RefMode;

fn line(vp: &mut Viewport) -> &mut LineTool {
    vp.active_tool_mut()
        .and_then(|t| t.as_any_mut().downcast_mut::<LineTool>())
        .expect("the line tool")
}

fn with_line() -> MainWindow {
    let mut w = window();
    w.activate_tool("line");
    w
}

#[test]
fn test_the_lock_goes_the_moment_the_segment_exists() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    line(vp).start_point = Some(DVec3::ZERO);
    vp.inference.axis_lock = Some(Axis::X);
    click(vp, 5.0, 5.0, Modifiers::NONE); // this one commits a segment
    assert_eq!(vp.inference.axis_lock, None);
}

#[test]
fn test_but_a_click_that_makes_nothing_keeps_it() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    vp.inference.axis_lock = Some(Axis::X);
    click(vp, 5.0, 5.0, Modifiers::NONE); // first click: nothing committed
    assert_eq!(vp.inference.axis_lock, Some(Axis::X));
    assert!(vp.tool_busy(), "the line is under way");
}

#[test]
fn test_the_lock_goes_when_the_line_is_finished() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    line(vp).start_point = Some(DVec3::X);
    vp.inference.axis_lock = Some(Axis::X);
    line(vp).start_point = None; // the chain closed (`_reset`)
    vp.release_axis_lock_after_operation(false);
    assert_eq!(vp.inference.axis_lock, None);
}

#[test]
fn test_switching_tool_drops_it_too() {
    let mut w = with_line();
    w.viewport.inference.axis_lock = Some(Axis::X);
    w.activate_tool("rectangle");
    assert_eq!(w.viewport.inference.axis_lock, None);
}

#[test]
fn test_a_typed_length_ends_the_operation_too() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    vp.inference.axis_lock = Some(Axis::Y);
    line(vp).start_point = None;
    vp.release_axis_lock_after_operation(false);
    assert_eq!(vp.inference.axis_lock, None);
}

#[test]
fn test_the_down_arrow_reference_is_left_alone() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    vp.inference.reference_mode = Some(RefMode::Parallel);
    line(vp).start_point = None;
    vp.release_axis_lock_after_operation(false);
    assert_eq!(vp.inference.reference_mode, Some(RefMode::Parallel));
    assert!(vp.release_constraints()); // …and Esc still does
    assert_eq!(vp.inference.reference_mode, None);
}

#[test]
fn test_esc_still_releases_it_mid_operation() {
    let mut w = with_line();
    let vp = &mut w.viewport;
    line(vp).start_point = Some(DVec3::ZERO);
    vp.inference.axis_lock = Some(Axis::Z);
    assert!(vp.release_constraints());
    assert_eq!(vp.inference.axis_lock, None);
}
