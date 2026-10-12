// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Esc escalates — value buffer → sticky constraints (axis lock / reference) →
//! the tool's action in progress → selection. The window's Esc action is what
//! fires.

use crate::common;
use xtrude_core::Axis;

use common::{v3, window};
use glam::DVec3;
use std::cell::Cell;
use std::rc::Rc;
use xtrude_app::tools::{Tool, ToolInfo};
use xtrude_app::viewport::{PickedEdge, Viewport};
use xtrude_core::snap::RefMode;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

/// Busy for `tool_busy` (an unfinished chain); records the cancel.
struct BusyTool {
    cancelled: Rc<Cell<bool>>,
}

impl Tool for BusyTool {
    xtrude_app::tool_any!();

    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo::new("busy", "Busy");
        &INFO
    }

    fn start_point(&self) -> Option<DVec3> {
        Some(DVec3::ZERO)
    }

    fn on_cancel(&mut self, _vp: &mut Viewport) {
        self.cancelled.set(true);
    }
}

#[test]
fn test_esc_releases_axis_lock_first() {
    let mut w = window();
    let cancelled = Rc::new(Cell::new(false));
    w.viewport.set_active_tool(Some(Box::new(BusyTool {
        cancelled: cancelled.clone(),
    })));
    w.viewport.inference.axis_lock = Some(Axis::X); // after: a tool switch drops the lock
    w.trigger("cancel_tool"); // 1st Esc: drop the lock only
    assert_eq!(w.viewport.inference.axis_lock, None);
    assert!(!cancelled.get()); // the chain in progress survives
    w.trigger("cancel_tool"); // 2nd Esc: now cancel the action
    assert!(cancelled.get());
}

#[test]
fn test_esc_releases_reference_mode() {
    let mut w = window();
    w.viewport.inference.reference_edge = Some(PickedEdge {
        a: DVec3::ZERO,
        b: DVec3::X,
        edge: None,
        in_group: false,
        context: None,
        group: None,
        soft: false,
        curve: None,
    });
    w.viewport.inference.reference_mode = Some(RefMode::Parallel);
    w.trigger("cancel_tool");
    assert!(
        w.viewport.inference.reference_mode.is_none()
            && w.viewport.inference.reference_edge.is_none()
    );
}

#[test]
fn test_esc_clears_value_buffer_before_constraints() {
    let mut w = window();
    w.viewport.set_value_buffer("2.5");
    w.viewport.inference.axis_lock = Some(Axis::Z);
    w.trigger("cancel_tool"); // 1st Esc: clear the typed value
    assert_eq!(w.viewport.value_box.buffer, "");
    assert_eq!(w.viewport.inference.axis_lock, Some(Axis::Z)); // the lock survives
    w.trigger("cancel_tool"); // 2nd Esc: release the lock
    assert_eq!(w.viewport.inference.axis_lock, None);
}

#[test]
fn test_esc_without_constraints_still_clears_selection() {
    let mut w = window();
    let f = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    w.viewport
        .scene
        .selection
        .insert(EntityRef::Face(MeshId::LOOSE, f));
    w.trigger("cancel_tool");
    assert!(w.viewport.scene.selection.is_empty());
}

#[test]
fn test_viewport_release_constraints_reports_whether_it_did() {
    let mut w = window();
    let vp = &mut w.viewport;
    assert!(!vp.release_constraints());
    vp.inference.axis_lock = Some(Axis::Y);
    assert!(vp.release_constraints());
    assert!(!vp.release_constraints());
}
