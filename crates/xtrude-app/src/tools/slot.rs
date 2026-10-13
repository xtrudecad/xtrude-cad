// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Slot tool: an obround, two half circles joined by straight sides, for
//! adjustable mounting holes. Click the first centre, the second centre,
//! then the width; or type `length;width` after the first click, the width
//! after the second. One face, its two arcs tagged as curves, one undo step.
//! The arcs follow the Circle's side rule: a full circle of `sides`
//! segments (24, `Ns` changes it), half of them per end.

use std::f64::consts::PI;

use glam::DVec3;
use xtrude_core::math::VecExt;
use xtrude_model::i18n::tr;
use xtrude_model::units::{fmt_len, fmt_pair};

use super::circle::commit_loop;
use super::{
    face_the_plane, world_per_pixel, Plane, PlaneLock, Rgba, SnapPrefs, Tool, ToolContext,
    ToolInfo, ToolValue, WorkPlane, PREVIEW_PX,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

/// The Circle's default side count.
pub const SLOT_SIDES: usize = 24;

/// A slot's outline and the two arcs it is made of (each with its ends).
#[derive(Clone, Debug, PartialEq)]
pub struct SlotOutline {
    /// The closed outline, counter-clockwise about the plane normal.
    pub points: Vec<DVec3>,
    /// The half circle about the second centre, then the first's.
    pub arcs: [Vec<DVec3>; 2],
}

/// The slot with centres `a` and `b` and half-width `r` on the plane of
/// `normal`, its ends cut in `sides / 2` segments each; `None` when the
/// centres meet or the width is nil.
pub fn slot_outline(
    a: DVec3,
    b: DVec3,
    r: f64,
    normal: DVec3,
    sides: usize,
) -> Option<SlotOutline> {
    let n = normal.unit_or_zero();
    let span = b - a;
    let u = (span - n * span.dot(n)).unit_or_zero();
    if u == DVec3::ZERO || r < 1e-9 || n == DVec3::ZERO {
        return None;
    }
    let b = a + u * span.dot(u);
    let v = n.cross(u);
    let half = sides.div_ceil(2).max(2);
    let arc = |c: DVec3, from: f64| -> Vec<DVec3> {
        (0..=half)
            .map(|k| {
                let t = from + PI * k as f64 / half as f64;
                c + (u * t.cos() + v * t.sin()) * r
            })
            .collect()
    };
    let end_b = arc(b, -0.5 * PI);
    let end_a = arc(a, 0.5 * PI);
    let points = end_b.iter().chain(end_a.iter()).copied().collect();
    Some(SlotOutline {
        points,
        arcs: [end_b, end_a],
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Waiting for the first centre.
    Idle,
    /// The first centre is down; the second follows the cursor.
    Second,
    /// Both centres are down; the cursor sets the width.
    Width,
}

pub struct SlotTool {
    /// The first centre.
    pub start_point: Option<DVec3>,
    /// The second centre.
    pub end_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub sides: usize,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
    wireframe: Option<Rgba>,
}

impl Default for SlotTool {
    fn default() -> SlotTool {
        SlotTool {
            start_point: None,
            end_point: None,
            hover_point: None,
            sides: SLOT_SIDES,
            work_plane: None,
            lock: PlaneLock::default(),
            wireframe: None,
        }
    }
}

impl SlotTool {
    pub fn new() -> SlotTool {
        SlotTool::default()
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.end_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
        self.wireframe = None;
        self.lock.clear_plane_lock();
    }

    fn step(&self) -> Step {
        match (self.start_point, self.end_point) {
            (None, _) => Step::Idle,
            (Some(_), None) => Step::Second,
            (Some(_), Some(_)) => Step::Width,
        }
    }

    fn plane_points(&self) -> Vec<DVec3> {
        match self.start_point {
            None => Vec::new(),
            Some(s) => std::iter::once(s)
                .chain(self.end_point.or(self.hover_point))
                .collect(),
        }
    }

    /// `(point, normal)` the slot is laid out on.
    pub fn drawing_plane(&self) -> Plane {
        self.lock
            .drawing_plane(self.work_plane, &self.plane_points())
    }

    /// `p` dropped onto the drawing plane.
    fn on_plane(&self, p: DVec3) -> DVec3 {
        let (o, n) = self.drawing_plane();
        let n = n.unit_or_zero();
        p - n * (p - o).dot(n)
    }

    /// The width the cursor at `p` asks for: twice its distance from the
    /// segment between the centres, in the plane.
    pub fn width_at(&self, p: DVec3) -> Option<f64> {
        let (a, b) = (self.start_point?, self.end_point?);
        let p = self.on_plane(p);
        let d = b - a;
        let t = ((p - a).dot(d) / d.length_squared().max(1e-18)).clamp(0.0, 1.0);
        Some(2.0 * (p - (a + d * t)).length())
    }

    /// The outline for both centres and a width.
    pub fn outline(&self, a: DVec3, b: DVec3, width: f64) -> Option<SlotOutline> {
        let (_, n) = self.drawing_plane();
        slot_outline(a, b, 0.5 * width, n, self.sides)
    }

    /// The second centre `length` from the first, toward the cursor.
    fn second_toward_cursor(&self, length: f64) -> Option<DVec3> {
        let a = self.start_point?;
        let dir = (self.on_plane(self.hover_point?) - a).unit_or_zero();
        (dir != DVec3::ZERO).then(|| a + dir * length)
    }

    /// Take the second centre; a centre on the first is refused.
    fn set_second(&mut self, vp: &mut Viewport, b: DVec3) -> bool {
        let Some(a) = self.start_point else {
            return false;
        };
        if (self.on_plane(b) - a).length() < 1e-6 {
            vp.flash_status(&tr("Slot needs two centres apart", &[]), 2500);
            return false;
        }
        // The plane is fixed from here: the width is read on it.
        self.work_plane = Some(self.drawing_plane());
        self.end_point = Some(self.on_plane(b));
        vp.update();
        true
    }

    fn commit(&mut self, vp: &mut Viewport, a: DVec3, b: DVec3, width: f64) -> bool {
        let Some(o) = self.outline(a, b, width) else {
            vp.flash_status(&tr("Slot needs a width", &[]), 2500);
            return false;
        };
        let (_, n) = self.drawing_plane();
        let pts = face_the_plane(&o.points, Some(n));
        let curves: Vec<(Vec<DVec3>, bool)> = o.arcs.into_iter().map(|c| (c, false)).collect();
        commit_loop(vp, &pts, &curves);
        self.reset();
        vp.update();
        true
    }

    fn cursor_preview(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(c) = self.hover_point else {
            return Vec::new();
        };
        let (_p, normal) = self.lock.preview_plane(vp, c);
        let (u, _v) = xtrude_core::axes::plane_axes(normal);
        let Some(scale) = world_per_pixel(vp, c, u) else {
            return Vec::new();
        };
        let r = 0.5 * PREVIEW_PX * scale;
        slot_outline(c - u * r, c + u * r, r, normal, self.sides)
            .map(|o| loop_lines(&o.points))
            .unwrap_or_default()
    }
}

fn loop_lines(pts: &[DVec3]) -> Vec<(DVec3, DVec3)> {
    let n = pts.len();
    (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect()
}

impl Tool for SlotTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("slot"),
            description: Some(
                "Draw a slot, two half circles joined by straight sides, from its two centres and its width.",
            ),
            ..ToolInfo::new("slot", "Slot")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some(
            match self.step() {
                Step::Idle => "Sides",
                Step::Second => "Length",
                Step::Width => "Width",
            }
            .into(),
        )
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        self.wireframe
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
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        match self.step() {
            Step::Idle => {
                self.start_point = Some(ctx.world);
                if self.work_plane.is_none() {
                    self.work_plane = self.lock.locked_work_plane(ctx.world);
                }
            }
            Step::Second => {
                self.set_second(vp, ctx.world);
            }
            Step::Width => {
                let (Some(a), Some(b), Some(w)) =
                    (self.start_point, self.end_point, self.width_at(ctx.world))
                else {
                    return;
                };
                self.commit(vp, a, b, w);
            }
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        self.wireframe = self.lock.lock_color();
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: self.step() == Step::Idle,
            comma_lists: true,
            segments: true,
            ..ValueSpec::default()
        }
    }

    /// Before the first click a side count (`24`, `24s`); then
    /// `length;width`, or the length alone; after the second click the
    /// width.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let sides = match (self.step(), value) {
            (_, ToolValue::Segments(n)) => Some(*n as i64),
            (Step::Idle, ToolValue::Num(x)) => Some(xtrude_core::math::round_even_int(*x)),
            _ => None,
        };
        if let Some(n) = sides {
            if n < 4 {
                return false;
            }
            self.sides = n as usize;
            vp.flash_status(&tr("{n} sides", &[("n", &n)]), 2500);
            vp.update();
            return true;
        }
        match (self.step(), value) {
            (Step::Second, &ToolValue::Pair(l, w)) => {
                let (Some(a), Some(b)) = (self.start_point, self.second_toward_cursor(l)) else {
                    return false;
                };
                if l <= 0.0 || w <= 0.0 || !self.set_second(vp, b) {
                    return false;
                }
                self.commit(vp, a, b, w)
            }
            (Step::Second, &ToolValue::Num(l)) => {
                let Some(b) = self.second_toward_cursor(l) else {
                    return false;
                };
                l > 0.0 && self.set_second(vp, b)
            }
            (Step::Width, &ToolValue::Num(w)) => {
                let (Some(a), Some(b)) = (self.start_point, self.end_point) else {
                    return false;
                };
                w > 0.0 && self.commit(vp, a, b, w)
            }
            _ => false,
        }
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(hover) = self.hover_point else {
            return Vec::new();
        };
        match (self.start_point, self.end_point) {
            (None, _) => self.cursor_preview(vp),
            (Some(a), None) => vec![(a, self.on_plane(hover))],
            (Some(a), Some(b)) => {
                let w = self.width_at(hover).unwrap_or(0.0);
                match self.outline(a, b, w) {
                    Some(o) => loop_lines(&o.points),
                    None => vec![(a, b)],
                }
            }
        }
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let a = self.start_point?;
        let hover = self.hover_point?;
        match self.end_point {
            None => {
                let b = self.on_plane(hover);
                Some((fmt_len((b - a).length()), Some((a + b) * 0.5)))
            }
            Some(b) => {
                let w = self.width_at(hover)?;
                Some((fmt_pair((b - a).length(), w), Some(hover)))
            }
        }
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        let started = self.start_point.is_some();
        self.lock.plane_lock_key(vp, key, started)
    }

    fn phase(&self) -> &'static str {
        match self.step() {
            Step::Idle => "idle",
            Step::Second => "started",
            Step::Width => "width",
        }
    }
    fn start_point(&self) -> Option<DVec3> {
        self.start_point
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
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs::axis_magnet(self.end_point.is_none())
    }
}
