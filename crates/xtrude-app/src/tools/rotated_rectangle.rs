// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotated Rectangle tool: a rectangle at any angle in any plane — the first
//! corner (which captures the plane), the second (the base edge, over a
//! protractor), then the width and its angle around the base edge (a second
//! protractor).

use crate::input::ScreenPt;
use std::f64::consts::PI;

use glam::DVec3;
use xtrude_core::axes::{self, plane_axes};
use xtrude_core::math::{round_even_int, VecExt};
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::i18n::tr;
use xtrude_model::units::{bare_number_scale, fmt_len, fmt_pair};

use super::{
    face_the_plane, Plane, PlaneLock, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane,
    DISC_PX, TICK_DEG,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::Viewport;

/// Below this the width makes no rectangle.
const MIN_WIDTH: f64 = 1e-6;
/// Degrees within which the angle sticks to flat / perpendicular.
const ANGLE_SNAP: f64 = 3.0;
/// Snaps that name a real point of the model: the width goes there.
const POINT_SNAPS: [&str; 9] = [
    "endpoint",
    "midpoint",
    "intersection",
    "on_edge",
    "close",
    "origin",
    "center",
    "centre",
    "on_line",
];

/// An angle typed in the value box: the parser read it as a length in the
/// document's unit, so undo that scale.
pub fn typed_degrees(value: f64) -> f64 {
    let s = bare_number_scale();
    if s != 0.0 {
        value / s
    } else {
        value
    }
}

fn round_to_tick(deg: f64) -> f64 {
    round_even_int(deg / TICK_DEG) as f64 * TICK_DEG
}

pub struct RotatedRectangleTool {
    pub start_point: Option<DVec3>,
    pub base_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
    /// The width's turn around the base edge, degrees (0 = flat in the
    /// plane, 90 = standing).
    pub angle: f64,
    pub locked: Option<DVec3>,
    pub disc_r: f64,
    pub near_disc: bool,
    pub held_angle: Option<f64>,
    pub held_dir: Option<DVec3>,
}

impl Default for RotatedRectangleTool {
    fn default() -> Self {
        RotatedRectangleTool::new()
    }
}

impl RotatedRectangleTool {
    pub fn new() -> RotatedRectangleTool {
        RotatedRectangleTool {
            start_point: None,
            base_point: None,
            hover_point: None,
            work_plane: None,
            lock: PlaneLock::default(),
            angle: 0.0,
            locked: None,
            disc_r: 1.0,
            near_disc: false,
            held_angle: None,
            held_dir: None,
        }
    }

    pub fn reset(&mut self) {
        self.start_point = None;
        self.base_point = None;
        self.work_plane = None;
        self.lock.clear_plane_lock();
        self.angle = 0.0;
        self.locked = None;
        self.near_disc = false;
        self.held_angle = None;
        self.held_dir = None;
    }

    /// The plane the rectangle lives in: the arrow-key lock, else the face
    /// under the click, else `None` (the ground).
    fn capture_plane(&self, vp: &mut Viewport, ctx: &ToolContext) -> Option<Plane> {
        if let Some(l) = self.lock.locked_work_plane(ctx.world) {
            return Some(l);
        }
        let hit = vp.pick_face_placement(ctx.screen.0, ctx.screen.1)?;
        let (_p, n) = vp.face_plane_world(&hit);
        if n.length_squared() < 0.5 {
            return None;
        }
        Some((ctx.world, n))
    }

    pub fn normal(&self) -> DVec3 {
        self.work_plane.map_or(DVec3::Z, |p| p.1.unit_or_zero())
    }

    /// In-plane unit vector perpendicular to the base edge (angle 0).
    pub fn perp(&self) -> DVec3 {
        let (Some(s), Some(b)) = (self.start_point, self.base_point) else {
            return DVec3::ZERO;
        };
        let e = b - s;
        if e.length() < 1e-9 {
            return DVec3::ZERO;
        }
        self.normal().cross(e.unit_or_zero()).unit_or_zero()
    }

    /// The width's direction for `angle` degrees around the base edge.
    pub fn width_dir(&self, angle: Option<f64>) -> DVec3 {
        let perp = self.perp();
        if perp.length() < 1e-6 {
            return DVec3::ZERO;
        }
        let a = angle.unwrap_or(self.angle).to_radians();
        (perp * a.cos() + self.normal() * a.sin()).unit_or_zero()
    }

    /// `(width, angle)` the cursor asks for around the base edge.
    pub fn width_and_angle(&self, cursor: DVec3, forced: Option<DVec3>) -> (f64, f64) {
        let perp = self.perp();
        let (Some(s), Some(b)) = (self.start_point, self.base_point) else {
            return (0.0, 0.0);
        };
        if perp.length() < 1e-6 {
            return (0.0, 0.0);
        }
        let edge = (b - s).unit_or_zero();
        let mut d = cursor - b;
        d -= edge * d.dot(edge);
        if let Some(f) = forced {
            d = f * d.dot(f);
        }
        let width = d.length();
        if width < 1e-9 {
            return (0.0, self.angle);
        }
        let mut angle = d.dot(self.normal()).atan2(d.dot(perp)).to_degrees();
        if self.near_disc && forced.is_none() {
            angle = round_to_tick(angle);
        }
        for target in [-180.0, -90.0, 0.0, 90.0, 180.0] {
            if (angle - target).abs() <= ANGLE_SNAP {
                angle = target;
                break;
            }
        }
        (width, angle)
    }

    /// The direction the viewport's axis lock imposes on the width.
    pub fn locked_dir(&self, vp: &Viewport) -> Option<DVec3> {
        let lock = vp.inference.axis_lock?;
        let (s, b) = (self.start_point?, self.base_point?);
        let edge = b - s;
        if edge.length() < 1e-9 {
            return None;
        }
        let edge = edge.unit_or_zero();
        let axis = axes::axis(lock);
        let d = axis - edge * axis.dot(edge);
        (d.length() > 1e-6).then(|| d.unit_or_zero())
    }

    pub fn corners(&self, width: f64, angle: Option<f64>) -> Vec<DVec3> {
        let dir = self.width_dir(angle);
        let (Some(s), Some(b)) = (self.start_point, self.base_point) else {
            return Vec::new();
        };
        if dir.length() < 1e-6 {
            return Vec::new();
        }
        let off = dir * width;
        vec![s, b, b + off, s + off]
    }

    fn commit(&mut self, vp: &mut Viewport, corners: Vec<DVec3>) {
        let plane = self
            .work_plane
            .map(|p| p.1)
            .or_else(|| vp.work_plane_normal());
        let corners = face_the_plane(&corners, plane);
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

    /// The cursor for the width, on the second protractor's plane.
    fn width_cursor(&self, vp: &Viewport, ctx: &ToolContext) -> DVec3 {
        if ctx
            .snap
            .as_ref()
            .is_some_and(|s| POINT_SNAPS.contains(&s.kind.as_str()))
        {
            return ctx.world;
        }
        let Some((o, d)) = vp.pixel_to_ray(ScreenPt::new(ctx.screen.0, ctx.screen.1)) else {
            return ctx.world;
        };
        let (Some(s), Some(b)) = (self.start_point, self.base_point) else {
            return ctx.world;
        };
        let n = (b - s).unit_or_zero();
        let denom = d.dot(n);
        if denom.abs() < 0.02 {
            return ctx.world;
        }
        let t = (b - o).dot(n) / denom;
        if t <= 0.0 {
            return ctx.world;
        }
        o + d * t
    }

    fn in_plane(&self, d: DVec3) -> DVec3 {
        let n = self.normal();
        d - n * d.dot(n)
    }

    /// The base edge's direction in the plane, degrees from its first axis.
    pub fn edge_angle(&self, d: DVec3) -> f64 {
        let (u, v) = plane_axes(self.normal());
        d.dot(v).atan2(d.dot(u)).to_degrees().rem_euclid(360.0)
    }

    fn dir_at(&self, deg: f64) -> DVec3 {
        let (u, v) = plane_axes(self.normal());
        let a = deg.to_radians();
        (u * a.cos() + v * a.sin()).unit_or_zero()
    }

    fn on_edge_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(start) = self.start_point else {
            return false;
        };
        let (mut length, mut dir) = match *value {
            ToolValue::Pair(l, a) => (l, self.dir_at(typed_degrees(a))),
            ToolValue::Num(l) => {
                let d = self.hover_point.map_or(DVec3::ZERO, |h| h - start);
                (
                    l,
                    if d.length() > 1e-9 {
                        d.unit_or_zero()
                    } else {
                        self.dir_at(0.0)
                    },
                )
            }
            _ => return false,
        };
        if length == 0.0 {
            return false;
        }
        if length < 0.0 {
            length = -length;
            dir = -dir;
        }
        self.base_point = Some(start + dir * length);
        if self.perp().length_squared() < 1e-12 {
            self.base_point = None;
            return false;
        }
        vp.update();
        true
    }

    fn disc_metrics(&mut self, vp: &Viewport, ctx: &ToolContext, centre: DVec3, axis: DVec3) {
        self.near_disc = false;
        let (u, _v) = plane_axes(axis);
        let (Some(p0), Some(p1)) = (vp.world_to_pixel(centre), vp.world_to_pixel(centre + u))
        else {
            return;
        };
        let px = (p1.x - p0.x).hypot(p1.y - p0.y);
        if px > 1e-6 {
            self.disc_r = DISC_PX / px;
        }
        self.near_disc = (ctx.screen.0 - p0.x).hypot(ctx.screen.1 - p0.y) <= DISC_PX * 1.25;
    }

    /// A protractor disc in the plane (u, v): rim and 15° ticks.
    pub fn disc(&self, centre: DVec3, u: DVec3, v: DVec3) -> Vec<(DVec3, DVec3)> {
        if u.length() < 1e-9 || v.length() < 1e-9 {
            return Vec::new();
        }
        let r = self.disc_r;
        let rim = |t: f64, k: f64| centre + (u * t.cos() + v * t.sin()) * (r * k);
        let n = 48;
        let pts: Vec<DVec3> = (0..n)
            .map(|k| rim(2.0 * PI * k as f64 / n as f64, 1.0))
            .collect();
        let mut segs: Vec<(DVec3, DVec3)> = (0..n).map(|k| (pts[k], pts[(k + 1) % n])).collect();
        let ticks = (360.0 / TICK_DEG) as usize;
        for k in 0..ticks {
            let deg = k as f64 * TICK_DEG;
            let t = deg.to_radians();
            let inner = if deg % 90.0 == 0.0 { 0.75 } else { 0.86 };
            segs.push((rim(t, inner), rim(t, 1.0)));
        }
        segs
    }
}

impl Tool for RotatedRectangleTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("rotated_rect"),
            description: Some(
                "Draw a rectangle at any angle and in any plane: a base edge, then the width.",
            ),
            ..ToolInfo::new("rotated_rect", "Rotated Rect")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some(
            if self.base_point.is_none() {
                "Length; angle"
            } else {
                "Width; angle"
            }
            .into(),
        )
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
        let Some(start) = self.start_point else {
            self.work_plane = self.capture_plane(vp, ctx);
            self.start_point = Some(ctx.world);
            return;
        };
        if self.base_point.is_none() {
            if (ctx.world - start).length() < 1e-6 {
                return;
            }
            self.base_point = match (self.held_dir, self.hover_point) {
                (Some(_), Some(h)) => Some(h),
                _ => Some(ctx.world),
            };
            self.held_dir = None;
            if self.perp().length_squared() < 1e-12 {
                self.base_point = None;
                vp.flash_status(
                    &tr(
                        "That edge is perpendicular to the drawing plane — lock another one with the arrow keys, or start on the face you want to draw on",
                        &[],
                    ),
                    5000,
                );
                vp.update();
            }
            return;
        }
        let locked = self.locked_dir(vp);
        let wc = self.width_cursor(vp, ctx);
        let (mut width, mut angle) = self.width_and_angle(wc, locked);
        if locked.is_some() && width < MIN_WIDTH {
            if let Some(h) = self.hover_point {
                (width, angle) = self.width_and_angle(h, locked);
            }
        }
        self.angle = angle;
        if width < MIN_WIDTH {
            vp.flash_status(
                &tr(
                    "The width is zero — move away from the base edge, or type the width (and the angle after a comma)",
                    &[],
                ),
                4000,
            );
            return;
        }
        let c = self.corners(width, None);
        if !c.is_empty() {
            self.commit(vp, c);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(if self.base_point.is_some() {
            self.width_cursor(vp, ctx)
        } else {
            ctx.world
        });
        self.locked = self.locked_dir(vp);
        if let (Some(start), None) = (self.start_point, self.base_point) {
            let n = self.normal();
            self.disc_metrics(vp, ctx, start, n);
            if self.near_disc {
                let d = self.in_plane(ctx.world - start);
                if d.length() > 1e-9 {
                    let deg = round_to_tick(self.edge_angle(d));
                    self.hover_point = Some(start + self.dir_at(deg) * d.length());
                }
            }
            if !ctx.modifiers.shift {
                self.held_dir = None;
            } else {
                let d = self.in_plane(self.hover_point.unwrap_or(start) - start);
                if self.held_dir.is_none() && d.length() > 1e-9 {
                    self.held_dir = Some(d.unit_or_zero());
                }
                if let Some(hd) = self.held_dir {
                    let along = (ctx.world - start).dot(hd);
                    self.hover_point = Some(start + hd * along);
                }
            }
        }
        if let (Some(start), Some(base)) = (self.start_point, self.base_point) {
            let edge = (base - start).unit_or_zero();
            self.disc_metrics(vp, ctx, base, edge);
            if !ctx.modifiers.shift {
                self.held_angle = None;
            } else if self.held_angle.is_none() {
                self.held_angle = Some(self.angle);
            }
            if let Some(ha) = self.held_angle {
                self.locked = Some(self.width_dir(Some(ha)));
            }
            let (w, a) = self.width_and_angle(self.hover_point.unwrap_or(base), self.locked);
            if w > MIN_WIDTH {
                self.angle = a;
            }
        }
        vp.update();
    }

    /// `3` = 3 wide at the current angle; `3;90` = 3 wide standing.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if self.start_point.is_none() {
            return false;
        }
        if self.base_point.is_none() {
            return self.on_edge_value(vp, value);
        }
        if self.hover_point.is_none() {
            return false;
        }
        let corners = match *value {
            ToolValue::Pair(w, a) => {
                if w <= 0.0 {
                    return false;
                }
                self.angle = typed_degrees(a);
                self.corners(w, None)
            }
            ToolValue::Num(w) => {
                if w == 0.0 {
                    return false;
                }
                self.corners(w, None)
            }
            _ => return false,
        };
        if !corners.is_empty() {
            self.commit(vp, corners);
        }
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        let started = self.start_point.is_some();
        self.lock.plane_lock_key(vp, key, started)
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(s), Some(h)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        let Some(b) = self.base_point else {
            let (u, v) = plane_axes(self.normal());
            let mut out = vec![(s, h)];
            out.extend(self.disc(s, u, v));
            return out;
        };
        let (w, a) = self.width_and_angle(h, self.locked);
        let c = self.corners(w, Some(a));
        let disc = self.disc(b, self.perp(), self.normal());
        if c.is_empty() {
            let mut out = vec![(s, b)];
            out.extend(disc);
            return out;
        }
        let mut out: Vec<(DVec3, DVec3)> = (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect();
        out.extend(disc);
        out
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (Some(s), Some(h)) = (self.start_point, self.hover_point) else {
            return None;
        };
        let Some(b) = self.base_point else {
            let d = h - s;
            let mut text = fmt_len(d.length());
            let flat = self.in_plane(d);
            if flat.length() > 1e-9 {
                text.push_str(&format!("   {:.0}°", self.edge_angle(flat)));
            }
            return Some((text, Some((s + h) * 0.5)));
        };
        let (w, angle) = self.width_and_angle(h, self.locked);
        let length = (b - s).length();
        let c = self.corners(w, Some(angle));
        let mid = if c.is_empty() { b } else { (s + c[2]) * 0.5 };
        let mut text = fmt_pair(length, w.abs());
        if angle.abs() > 0.05 {
            text.push_str(&format!("   {angle:.0}°"));
        }
        Some((text, Some(mid)))
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
        SnapPrefs::axis_magnet(self.base_point.is_none())
    }
}
