// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The contextual Shift lock takes the inferred axis exactly, keeping its
//! sign.

use crate::common;
use xtrude_core::Axis;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::line::LineTool;
use xtrude_core::snap::{SnapKind, SnapResult, COLOR_AXIS_X};

fn capture(snap_point: DVec3) -> DVec3 {
    let mut vp = stub();
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    vp.tool_mut::<LineTool>().unwrap().start_point = Some(DVec3::new(2.0, 3.0, 4.0));
    let mut s = SnapResult::new(snap_point, SnapKind::AxisInference, COLOR_AXIS_X);
    s.axis = Some(Axis::X);
    vp.inference.last_snap = Some(s);
    vp.capture_shift_lock();
    vp.inference.shift_lock.expect("a lock").0
}

#[test]
fn test_shift_lock_uses_exact_positive_inferred_axis() {
    assert_eq!(capture(DVec3::new(5.0, 3.01, 4.0)), DVec3::X);
}

#[test]
fn test_shift_lock_preserves_negative_inferred_axis_direction() {
    assert_eq!(capture(DVec3::new(-1.0, 3.01, 4.0)), -DVec3::X);
}
