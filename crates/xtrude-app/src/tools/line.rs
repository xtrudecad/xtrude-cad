// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Line tool: click points to draw edges; a closed planar loop becomes a face.
//!
//! The first click starts a chain, each next click commits a segment and
//! chains on; snapping to the chain's first point ("close") finishes it.
//! Esc cancels without committing the pending segment.

use glam::DVec3;
use xtrude_core::math::VecExt;
use xtrude_model::edits::build_add_edge;

use super::{Plane, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane};
use crate::tool_any;
use crate::viewport::Viewport;

#[derive(Default)]
pub struct LineTool {
    pub start_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub chain_first_point: Option<DVec3>,
    /// The chain's vertices, first click first.
    pub chain_vertices: Vec<DVec3>,
    /// The face the chain was started on.
    pub work_plane: Option<Plane>,
}

impl LineTool {
    pub fn new() -> LineTool {
        LineTool::default()
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.chain_first_point = None;
        self.chain_vertices.clear();
        self.work_plane = None;
    }

    /// The command for a new edge: welds, splits what it crosses, and
    /// closes faces.
    fn commit_edge(vp: &mut Viewport, a: DVec3, b: DVec3) {
        let eye = Some(vp.camera.eye());
        let cmd = build_add_edge(&vp.scene, a, b, true, eye);
        vp.execute_boxed(cmd);
    }
}

impl Tool for LineTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("line"),
            shortcut: Some("L"),
            description: Some("Draw edges point by point; closing a loop makes a face."),
            ..ToolInfo::new("line", "Line")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Length".into())
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let clicked = ctx.world;
        let Some(start) = self.start_point else {
            self.start_point = Some(clicked);
            self.chain_first_point = Some(clicked);
            self.chain_vertices = vec![clicked];
            return;
        };
        if (clicked - start).length() < 1e-9 {
            // The start itself: nothing to draw.
            return;
        }
        Self::commit_edge(vp, start, clicked);
        if ctx.snap_kind() == "close" {
            self.reset();
        } else {
            self.chain_vertices.push(clicked);
            self.start_point = Some(clicked);
        }
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    /// Ctrl+Z mid-chain: the segment just drawn is undone and the
    /// chain steps back; with only the first point placed it is let go.
    fn on_undo(&mut self, vp: &mut Viewport) -> Option<bool> {
        self.start_point?;
        if self.chain_vertices.len() > 1 {
            if !vp.undo() {
                self.reset();
                vp.update();
                return Some(true);
            }
            self.chain_vertices.pop();
            self.start_point = self.chain_vertices.last().copied();
            if self.chain_vertices.len() == 1 {
                self.chain_first_point = self.start_point;
            }
        } else {
            self.reset();
        }
        vp.update();
        Some(true)
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        match (self.start_point, self.hover_point) {
            (Some(s), Some(h)) => vec![(s, h)],
            _ => Vec::new(),
        }
    }

    /// A length along the rubber band (negative: the other way), or a
    /// `dx;dy;dz` delta from the start.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(start) = self.start_point else {
            return false;
        };
        let end = match *value {
            ToolValue::Triple(dx, dy, dz) => start + DVec3::new(dx, dy, dz),
            ToolValue::Num(v) => {
                let Some(h) = self.hover_point else {
                    return false;
                };
                if v.abs() < 1e-9 {
                    return false;
                }
                let delta = h - start;
                if delta.length() < 1e-9 {
                    return false;
                }
                start + delta.unit_or_zero() * v
            }
            _ => return false,
        };
        Self::commit_edge(vp, start, end);
        self.chain_vertices.push(end);
        self.start_point = Some(end);
        self.hover_point = Some(end);
        vp.update();
        true
    }

    fn start_point(&self) -> Option<DVec3> {
        self.start_point
    }
    fn chain_first_point(&self) -> Option<DVec3> {
        self.chain_first_point
    }
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        Some(WorkPlane {
            plane: self.work_plane,
            lock: None,
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.work_plane = plane;
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs::axis_magnet(true)
    }
}
