// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Section Plane tool.
//!
//! The plane glyph follows the cursor, aligned to the face underneath.
//! Shift LOCKS the current orientation; the arrows orient the normal to an
//! axis (Up = blue, Right = red, Left = green, Down = back to the face).
//! A click places the plane as the ACTIVE cut, the window asks for a name
//! and symbol, and the tool hands back to Select.

use glam::DVec3;
use xtrude_core::axes::{self, plane_axes};
use xtrude_core::snap::face_plane_world;
use xtrude_core::snap::COLOR_REFERENCE;
use xtrude_core::Axis;
use xtrude_model::commands::PlaceSectionPlaneCommand;
use xtrude_model::i18n::tr;
use xtrude_model::section::{next_symbol, SectionPlane};

use super::{PlaneLock, Rgba, Tool, ToolContext, ToolInfo};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::{Viewport, ViewportEvent};

pub struct SectionPlaneTool {
    pub hover_point: Option<DVec3>,
    normal: DVec3,
    /// Arrow-key orientation lock.
    pub axis_pick: Option<Axis>,
    shift_normal: Option<DVec3>,
    /// The camera's eye at the last hover (the side a new plane hides).
    eye: Option<DVec3>,
}

impl Default for SectionPlaneTool {
    fn default() -> Self {
        SectionPlaneTool {
            hover_point: None,
            normal: DVec3::Z,
            axis_pick: None,
            shift_normal: None,
            eye: None,
        }
    }
}

impl SectionPlaneTool {
    pub fn new() -> SectionPlaneTool {
        SectionPlaneTool::default()
    }

    /// Set the face-inferred normal (what the last hover read).
    pub fn set_normal(&mut self, n: DVec3) {
        self.normal = n;
    }

    /// The eye a new plane turns toward; `None` without a camera (a
    /// stand-in with no eye of its own).
    fn camera_eye(vp: &Viewport) -> Option<DVec3> {
        vp.probe().eye(vp)
    }

    pub fn current_normal(&self) -> DVec3 {
        if let Some(n) = self.shift_normal {
            return n;
        }
        if let Some(a) = self.axis_pick {
            return self.toward_camera(axes::axis(a));
        }
        self.normal
    }

    /// The cut hides the normal's side, and a freshly placed plane hides
    /// the side the CAMERA is on: an axis lock or the ground default turns
    /// to face the viewer like a face normal does.
    fn toward_camera(&self, n: DVec3) -> DVec3 {
        let (Some(eye), Some(h)) = (self.eye, self.hover_point) else {
            return n;
        };
        if n.dot(eye - h) < 0.0 {
            -n
        } else {
            n
        }
    }
}

impl Tool for SectionPlaneTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("section"),
            description: Some("Place a plane that cuts the model open to show its inside."),
            ..ToolInfo::new("section", "Section Plane")
        };
        &INFO
    }
    /// The floating plane wears the inference colours: red/green/
    /// blue square to an axis, magenta on any other plane.
    fn wireframe_color(&self) -> Option<Rgba> {
        PlaneLock::plane_color(self.current_normal()).or(Some([
            COLOR_REFERENCE[0],
            COLOR_REFERENCE[1],
            COLOR_REFERENCE[2],
            1.0,
        ]))
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.eye = Self::camera_eye(vp);
        self.hover_point = None;
        self.axis_pick = None;
        self.shift_normal = None;
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.hover_point = None;
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        self.eye = Self::camera_eye(vp);
        let axis = match key {
            Key::Down => {
                self.axis_pick = None;
                vp.update();
                return true;
            }
            Key::Up => Axis::Z,
            Key::Right => Axis::X,
            Key::Left => Axis::Y,
            _ => return false,
        };
        self.axis_pick = Some(axis);
        vp.update();
        true
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.eye = Self::camera_eye(vp);
        self.hover_point = Some(ctx.world);
        let shift = ctx.modifiers.shift;
        if !shift {
            self.shift_normal = None;
        }
        if shift {
            if self.shift_normal.is_none() {
                self.shift_normal = Some(self.current_normal());
            }
        } else if self.axis_pick.is_none() {
            match vp.pick_face_any(ctx.screen.0, ctx.screen.1) {
                Some(hit) => {
                    let mesh = vp.scene.doc.mesh(hit.mesh);
                    if mesh.has_face(hit.face) {
                        let (_p, n) = face_plane_world(mesh, hit.face, hit.xform.as_ref());
                        self.normal = n.normalize_or(DVec3::Z);
                    }
                }
                // ground: a horizontal cut
                None => self.normal = self.toward_camera(DVec3::Z),
            }
        }
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let n = self.current_normal();
        let planes = &vp.scene.doc.section_planes;
        let count = planes.len() + 1;
        let plane = SectionPlane::new(
            ctx.world,
            n,
            &tr("Section {n}", &[("n", &count)]),
            &next_symbol(planes),
            false,
            None,
        );
        let id = plane.id;
        // A new plane is placed to be SEEN: cuts and planes switched off
        // come back on.
        if !(vp.scene.show_section_cuts && vp.scene.show_section_planes) {
            vp.scene.show_section_cuts = true;
            vp.scene.show_section_planes = true;
        }
        vp.execute(&mut PlaceSectionPlaneCommand::new(plane));
        vp.outbox.events.push(ViewportEvent::PromptSectionName(id));
        vp.flash_status(
            &tr(
                "Section plane placed — double-click toggles the cut; Move/Rotate reposition it",
                &[],
            ),
            4000,
        );
        vp.update();
        // One plane per pick-up, then back to Select.
        vp.outbox
            .events
            .push(ViewportEvent::ActivateTool("select".into()));
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(c) = self.hover_point else {
            return Vec::new();
        };
        let n = self.current_normal();
        let (u, v) = plane_axes(n);
        let r = 1.2;
        let quad = [
            c - u * r - v * r,
            c + u * r - v * r,
            c + u * r + v * r,
            c - u * r + v * r,
        ];
        let mut segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (quad[i], quad[(i + 1) % 4])).collect();
        // A short normal whisker shows which side will be CUT AWAY.
        segs.push((c, c + n * (r * 0.5)));
        segs
    }
}
