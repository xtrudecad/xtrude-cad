// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Dimension tool: place a static linear dimension in three clicks — first
//! endpoint, second endpoint, then slide the dimension line off the segment
//! and click to place it (aligned, or linear along an axis when pulled past
//! the ends).

use glam::DVec3;
use xtrude_core::snap::project_to_view_plane;
use xtrude_core::Axis;
use xtrude_model::commands::AddDimensionCommand;
use xtrude_model::dimension::Dimension;
use xtrude_model::units::fmt_len;

use super::{Plane, SnapPrefs, Tool, ToolContext, ToolInfo, WorkPlane};
use crate::tool_any;
use crate::viewport::Viewport;

#[derive(Default)]
pub struct DimensionTool {
    pub a: Option<DVec3>,
    pub b: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
}

impl DimensionTool {
    pub fn new() -> DimensionTool {
        DimensionTool::default()
    }

    fn reset(&mut self) {
        self.a = None;
        self.b = None;
        self.work_plane = None;
    }

    /// The second endpoint is a direction from the first; the
    /// placement click is not.
    fn magnet_on(&self) -> bool {
        self.b.is_none()
    }

    /// In a standard axis-aligned view, pull `p` into the first point's
    /// view plane so the dimension reads the frontal span. No-op before
    /// the first point, in oblique views, and without a camera.
    fn frontal(&self, vp: &Viewport, p: DVec3) -> DVec3 {
        let Some(a) = self.a else {
            return p;
        };
        let Some(camera) = vp.probe().camera(vp) else {
            return p;
        };
        project_to_view_plane(p, a, camera.target - camera.eye(), 0.97)
    }

    /// `(a, b, offset, axis)` of the dimension the cursor would place.
    fn proposed(&self, cursor: DVec3) -> Option<(DVec3, DVec3, DVec3, Option<Axis>)> {
        let (a, b) = (self.a?, self.b?);
        let (offset, axis) = Dimension::placement_for_cursor(a, b, cursor);
        Some((a, b, offset, axis))
    }
}

impl Tool for DimensionTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("dimension"),
            shortcut: Some("D"),
            description: Some("Place a dimension that shows the distance between two points."),
            ..ToolInfo::new("dimension", "Dimension")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs::axis_magnet(self.magnet_on())
    }
    tool_any!();

    fn start_point(&self) -> Option<DVec3> {
        self.a
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

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(self.frontal(vp, ctx.world));
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let p = ctx.world;
        let Some(a) = self.a else {
            self.a = Some(p);
            return;
        };
        if self.b.is_none() {
            let p = self.frontal(vp, p);
            if (p - a).length() < 1e-6 {
                return; // need two distinct endpoints
            }
            self.b = Some(p);
            return;
        }
        // Third click: place the dimension where the cursor asks.
        if let Some((a, b, offset, axis)) = self.proposed(p) {
            let d = Dimension::new(a, b, offset).with_axis(axis);
            vp.execute(&mut AddDimensionCommand::new(d));
        }
        self.reset();
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(hover), Some(a)) = (self.hover_point, self.a) else {
            return Vec::new();
        };
        match self.proposed(hover) {
            None => vec![(a, hover)], // the measuring span
            Some((a, b, off, axis)) => {
                let (ap, bp) = Dimension::line_points_of(a, b, off, axis);
                vec![(a, ap), (b, bp), (ap, bp)] // extension + dimension line
            }
        }
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (hover, a) = (self.hover_point?, self.a?);
        match self.proposed(hover) {
            None => Some((fmt_len((hover - a).length()), Some((a + hover) * 0.5))),
            Some((a, b, off, axis)) => {
                let (ap, bp) = Dimension::line_points_of(a, b, off, axis);
                Some((
                    fmt_len(Dimension::value_of(a, b, axis)),
                    Some((ap + bp) * 0.5),
                ))
            }
        }
    }
}
