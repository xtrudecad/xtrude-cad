// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Change Axes: give a group or component new local axes in three clicks
//! — the new ORIGIN, a point along the new RED axis, a point giving the GREEN
//! direction (squared to red; blue is what is left, right-handed). Nothing
//! moves in the world.

use glam::{DMat4, DVec3};
use xtrude_core::axes::frame_matrix;
use xtrude_core::math::VecExt;
use xtrude_model::commands::ChangeAxesCommand;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;

use super::{Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::{Viewport, ViewportEvent};

#[derive(Default)]
pub struct ChangeAxesTool {
    /// The group or component whose axes change — set by whoever
    /// activates the tool (the context menu).
    pub target: Option<GroupId>,
    /// The new origin.
    pub start_point: Option<DVec3>,
    pub red_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
}

impl ChangeAxesTool {
    pub fn new() -> ChangeAxesTool {
        ChangeAxesTool::default()
    }

    pub fn for_target(target: GroupId) -> ChangeAxesTool {
        ChangeAxesTool {
            target: Some(target),
            ..ChangeAxesTool::default()
        }
    }

    /// `(x, y, z)` for the clicks so far and a green point, or `None` when
    /// they do not make a frame.
    pub fn axes_for(&self, green_at: Option<DVec3>) -> Option<(DVec3, DVec3, DVec3)> {
        let (o, r) = (self.start_point?, self.red_point?);
        let x = r - o;
        if x.length() < 1e-9 {
            return None;
        }
        let x = x.unit_or_zero();
        let g = green_at? - o;
        let y = g - x * g.dot(x);
        if y.length() < 1e-6 {
            return None;
        }
        let y = y.unit_or_zero();
        let z = x.cross(y).unit_or_zero();
        Some((x, y, z))
    }

    pub fn frame(&self, green_at: DVec3) -> Option<DMat4> {
        let (x, y, z) = self.axes_for(Some(green_at))?;
        Some(frame_matrix(self.start_point?, x, y, z))
    }
}

impl Tool for ChangeAxesTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo::new("change_axes", "Change Axes");
        &INFO
    }
    tool_any!();

    fn start_point(&self) -> Option<DVec3> {
        self.start_point
    }

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.start_point = None;
        self.red_point = None;
        self.hover_point = None;
        if self.target.is_none() {
            vp.flash_status(&tr("Select one group or component first", &[]), 2500);
        }
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.start_point = None;
        self.red_point = None;
        self.hover_point = None;
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.start_point = None;
        self.red_point = None;
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(target) = self.target else {
            return;
        };
        let p = ctx.world;
        let Some(o) = self.start_point else {
            self.start_point = Some(p);
            vp.flash_status(&tr("Click along the new red axis", &[]), 2500);
            return;
        };
        if self.red_point.is_none() {
            if (p - o).length() < 1e-6 {
                return;
            }
            self.red_point = Some(p);
            vp.flash_status(&tr("Click to set the green direction", &[]), 2500);
            return;
        }
        let Some(frame) = self.frame(p) else {
            vp.flash_status(
                &tr(
                    "The green direction cannot run along red — click off the red axis",
                    &[],
                ),
                4000,
            );
            return;
        };
        vp.execute(&mut ChangeAxesCommand {
            group: target,
            frame,
        });
        vp.flash_status(&tr("Axes changed", &[]), 2500);
        self.target = None;
        self.start_point = None;
        self.red_point = None;
        vp.outbox
            .events
            .push(ViewportEvent::ActivateTool("select".into()));
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(o), Some(h)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        let Some(r) = self.red_point else {
            return vec![(o, h)];
        };
        let Some((x, y, z)) = self.axes_for(Some(h)) else {
            return vec![(o, r)];
        };
        let len = (r - o).length().max(0.5);
        vec![(o, o + x * len), (o, o + y * len), (o, o + z * len)]
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        self.start_point?;
        let h = self.hover_point?;
        let text = if self.red_point.is_none() {
            tr("Red axis", &[])
        } else {
            tr("Green axis", &[])
        };
        Some((text, Some(h)))
    }
}
