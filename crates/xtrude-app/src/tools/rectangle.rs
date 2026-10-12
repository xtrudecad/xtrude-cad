// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rectangle tool: two clicks define opposite corners on the work plane; one
//! undo step.
//!
//! No axis magnet, deliberately: the second click is the
//! opposite corner, and a corner pulled onto the axis through the first is
//! a rectangle of zero height.

use glam::DVec3;
use xtrude_core::axes;
use xtrude_core::math::VecExt;
use xtrude_core::Axis;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_pair;

use super::{
    face_the_plane, plane_lock_name, world_per_pixel, Plane, PlaneLock, Rgba, Tool, ToolContext,
    ToolInfo, ToolValue, WorkPlane, PREVIEW_PX,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

/// Two orthonormal in-plane axes from a plane normal: `u` is world +X
/// projected on the plane (+Y when the normal is nearly X), `v = n × u`.
pub fn rect_plane_axes(normal: DVec3) -> (DVec3, DVec3) {
    if !axes::is_world() {
        return axes::plane_axes(normal);
    }
    let n = normal.unit_or_zero();
    let mut r = DVec3::X;
    let mut u = r - n * r.dot(n);
    if u.length() < 0.1 {
        r = DVec3::Y;
        u = r - n * r.dot(n);
    }
    let u = u.unit_or_zero();
    let v = n.cross(u).unit_or_zero();
    (u, v)
}

fn copysign(m: f64, s: f64) -> f64 {
    m.copysign(s)
}

#[derive(Default)]
pub struct RectangleTool {
    pub start_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    /// Ctrl: the first click is the CENTRE.
    pub from_center: bool,
    pub chain_first_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
    wireframe: Option<Rgba>,
    pub shift_square_lock: bool,
    pub arrow_square_lock: bool,
}

/// Within this fraction of the longer side, the sides count as equal.
pub const SQUARE_TOL: f64 = 0.04;

impl RectangleTool {
    pub fn new() -> RectangleTool {
        RectangleTool::default()
    }

    pub fn reset(&mut self) {
        self.start_point = None;
        self.chain_first_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
        self.wireframe = None;
        self.shift_square_lock = false;
        self.arrow_square_lock = false;
        self.lock.clear_plane_lock();
    }

    pub fn square_locked(&self) -> bool {
        self.shift_square_lock || self.arrow_square_lock
    }

    fn plane_points(&self) -> Vec<DVec3> {
        match self.start_point {
            None => Vec::new(),
            Some(s) => std::iter::once(s).chain(self.hover_point).collect(),
        }
    }

    /// `(point, normal)` the rectangle is laid out on.
    pub fn drawing_plane(&self) -> Plane {
        self.lock
            .drawing_plane(self.work_plane, &self.plane_points())
    }

    /// In-plane horizontal / vertical axes of the drawing plane.
    pub fn axes(&self) -> (DVec3, DVec3) {
        rect_plane_axes(self.drawing_plane().1)
    }

    /// Signed (width, height) of the rectangle spanning `a`–`b`.
    pub fn dimensions(&self, a: DVec3, b: DVec3) -> (f64, f64) {
        let (u, v) = self.axes();
        let d = b - a;
        (d.dot(u), d.dot(v))
    }

    /// The opposite corner nudged to a perfect square when within
    /// `SQUARE_TOL` (or `force`), and whether it is one.
    pub fn square_corner(&self, a: DVec3, b: DVec3, force: bool) -> (DVec3, bool) {
        let (u, v) = self.axes();
        let (du, dv) = self.dimensions(a, b);
        let (adu, adv) = (du.abs(), dv.abs());
        if adu < 1e-6 || adv < 1e-6 {
            return (b, false);
        }
        if force || (adu - adv).abs() <= SQUARE_TOL * adu.max(adv) {
            let side = adu.max(adv);
            return (a + u * copysign(side, du) + v * copysign(side, dv), true);
        }
        (b, false)
    }

    /// The two opposite corners the cursor asks for.
    pub fn span(&self, cursor: DVec3) -> (DVec3, DVec3) {
        let start = self.start_point.unwrap_or(cursor);
        let (far, _) = self.square_corner(start, cursor, self.square_locked());
        if self.from_center {
            (start * 2.0 - far, far)
        } else {
            (start, far)
        }
    }

    pub fn corners(&self, a: DVec3, b: DVec3) -> Vec<DVec3> {
        let (u, v) = self.axes();
        let d = b - a;
        let (du, dv) = (d.dot(u), d.dot(v));
        vec![a, a + u * du, a + u * du + v * dv, a + v * dv]
    }

    fn commit_rect(&mut self, vp: &mut Viewport, corners: Vec<DVec3>) {
        let (u, v) = self.axes();
        let corners = face_the_plane(&corners, Some(u.cross(v)));
        let segs: Vec<(DVec3, DVec3)> =
            (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).collect();
        let cmd = build_add_edges(
            &vp.scene,
            &segs,
            false,
            vec![Box::new(AddFaceCommand::new(corners))],
            None,
        );
        vp.execute_boxed(cmd);
        self.reset();
        vp.update();
    }

    /// Why the rectangle came out flat, in the words that help.
    pub fn degenerate_reason(&self, world: DVec3) -> String {
        let plain = || tr("Rectangle needs two sides — pick the opposite corner", &[]);
        let (Some((origin, normal)), Some(start)) = (self.work_plane, self.start_point) else {
            return plain();
        };
        let n = normal.unit_or_zero();
        if (world - origin).dot(n).abs() < 1e-3 {
            return plain();
        }
        let span = world - start;
        let axis = Axis::ALL
            .into_iter()
            .min_by(|a, b| {
                span.dot(axes::axis(*a))
                    .abs()
                    .total_cmp(&span.dot(axes::axis(*b)).abs())
            })
            .unwrap_or(Axis::Z);
        let arrow = match axis {
            Axis::X => tr("the Right arrow", &[]),
            Axis::Y => tr("the Left arrow", &[]),
            _ => tr("the Up arrow", &[]),
        };
        tr(
            "That corner is off the drawing plane, so the rectangle has no width. Press {arrow} for the {plane} plane.",
            &[("arrow", &arrow), ("plane", &plane_lock_name(axis))],
        )
    }

    fn cursor_preview(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(c) = self.hover_point else {
            return Vec::new();
        };
        let (_p, normal) = self.lock.preview_plane(vp, c);
        let (u, v) = rect_plane_axes(normal);
        let Some(scale) = world_per_pixel(vp, c, u) else {
            return Vec::new();
        };
        let h = 0.5 * PREVIEW_PX * scale;
        let pts = [
            c + u * h + v * h,
            c - u * h + v * h,
            c - u * h - v * h,
            c + u * h - v * h,
        ];
        (0..4).map(|i| (pts[i], pts[(i + 1) % 4])).collect()
    }

    fn set_from_center(&mut self, vp: &mut Viewport, on: bool) {
        self.from_center = on;
        vp.refresh_tool_cursor();
    }
}

impl Tool for RectangleTool {
    fn info(&self) -> &'static ToolInfo {
        static RECTANGLE: ToolInfo = ToolInfo {
            icon: Some("rectangle"),
            shortcut: Some("R"),
            description: Some("Draw a rectangle from two opposite corners."),
            ..ToolInfo::new("rectangle", "Rectangle")
        };
        // From the centre: the same tool, its own icon.
        static FROM_CENTER: ToolInfo = ToolInfo {
            icon: Some("rectangle_center"),
            ..RECTANGLE
        };
        if self.from_center {
            &FROM_CENTER
        } else {
            &RECTANGLE
        }
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Dimensions".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        self.wireframe
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        self.set_from_center(vp, false);
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        let Some(_) = self.start_point else {
            self.start_point = Some(ctx.world);
            if self.work_plane.is_none() {
                self.work_plane = self.lock.locked_work_plane(ctx.world);
            }
            return;
        };
        self.hover_point = Some(ctx.world);
        let (anchor, far) = self.span(ctx.world);
        let (du, dv) = self.dimensions(anchor, far);
        if du.abs() < 1e-6 || dv.abs() < 1e-6 {
            let msg = self.degenerate_reason(ctx.world);
            vp.flash_status(&msg, 2500);
            return;
        }
        let c = self.corners(anchor, far);
        self.commit_rect(vp, c);
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        let square = self
            .start_point
            .is_some_and(|s| self.square_corner(s, ctx.world, false).1);
        if !ctx.modifiers.shift {
            self.shift_square_lock = false;
        } else if square {
            self.shift_square_lock = true;
        }
        self.wireframe = self.lock.lock_color();
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            comma_lists: true,
            ..ValueSpec::default()
        }
    }

    /// `3;2` lays a 3 × 2 rectangle toward the cursor; a single number a
    /// square when the square lock is on.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return false;
        };
        let (w, h) = match *value {
            ToolValue::Num(x) => {
                if !self.square_locked() {
                    return false;
                }
                (x, x)
            }
            ToolValue::Pair(a, b) => (a, b),
            _ => return false,
        };
        if w <= 0.0 || h <= 0.0 {
            return false;
        }
        let (u, v) = self.axes();
        let (du, dv) = self.dimensions(start, hover);
        let su = if du < 0.0 { -1.0 } else { 1.0 };
        let sv = if dv < 0.0 { -1.0 } else { 1.0 };
        let (anchor, far) = if self.from_center {
            let half = u * (su * w * 0.5) + v * (sv * h * 0.5);
            (start - half, start + half)
        } else {
            (start, start + u * (su * w) + v * (sv * h))
        };
        let c = self.corners(anchor, far);
        self.commit_rect(vp, c);
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(hover) = self.hover_point else {
            return Vec::new();
        };
        if self.start_point.is_none() {
            return self.cursor_preview(vp);
        }
        let (anchor, far) = self.span(hover);
        let is_square = self.square_corner(anchor, far, false).1;
        let c = self.corners(anchor, far);
        let mut lines = vec![(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])];
        if is_square {
            lines.push((c[0], c[2]));
        }
        lines
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (Some(_), Some(hover)) = (self.start_point, self.hover_point) else {
            return None;
        };
        let (anchor, far) = self.span(hover);
        let is_square = self.square_corner(anchor, far, false).1;
        let (du, dv) = self.dimensions(anchor, far);
        let mut text = fmt_pair(du.abs(), dv.abs());
        if is_square {
            text.push_str(&format!("  ({})", tr("Square", &[])));
        }
        Some((text, Some((anchor + far) * 0.5)))
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if key == Key::Control {
            let on = !self.from_center;
            self.set_from_center(vp, on);
            let msg = tr(
                if on {
                    "Rectangle from the centre"
                } else {
                    "Rectangle from a corner"
                },
                &[],
            );
            vp.flash_status(&msg, 2500);
            return true;
        }
        if key == Key::Down {
            if let Some(start) = self.start_point {
                if self.arrow_square_lock {
                    self.arrow_square_lock = false;
                    vp.update();
                    return true;
                }
                if self
                    .hover_point
                    .is_some_and(|h| self.square_corner(start, h, false).1)
                {
                    self.arrow_square_lock = true;
                    vp.update();
                    return true;
                }
            }
        }
        let started = self.start_point.is_some();
        self.lock.plane_lock_key(vp, key, started)
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
            lock: Some(&self.lock),
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.work_plane = plane;
    }
}
