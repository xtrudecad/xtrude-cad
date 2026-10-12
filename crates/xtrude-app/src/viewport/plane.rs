// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Pixel → world: the work plane the cursor is read on.

use crate::input::ScreenPt;
use glam::{DVec3, DVec4};
use xtrude_core::math::VecExt;
use xtrude_core::snap;
use xtrude_model::entity::EntityRef;
use xtrude_model::guide::{Guide, GUIDE_HALF_LEN};

use super::Viewport;
use crate::tools::Plane;

/// A ray meeting the work plane below this angle gives a point nobody
/// asked for.
pub const GRAZING_PLANE_DEG: f64 = 6.0;
/// Above this camera tilt the work plane stays horizontal.
pub const HORIZON_PITCH_THRESHOLD_DEG: f64 = 15.0;
/// Eye depth (m) a segment is cut at when it runs behind the camera.
const CLIP_W: f64 = 1e-3;

impl Viewport {
    /// Pixel → world hit on the current work plane.
    pub fn world_from_pixel(&mut self, x: f64, y: f64) -> Option<DVec3> {
        let (origin, direction) = self.pixel_to_ray(ScreenPt::new(x, y))?;
        let (pp, pn) = self.current_work_plane(Some((x, y)));
        self.inference.last_work_plane = Some((pp, pn));
        if let Some(hit) = self.ray_plane(origin, direction, pp, pn) {
            return Some(hit);
        }
        // The ray only grazes the plane: if a real face IS under the
        // cursor, believe the face.
        let hit = self.pick_face_any(x, y)?;
        let (fp, fnorm) = self.face_plane_world(&hit);
        self.ray_plane(origin, direction, fp, fnorm)
    }

    /// The normal of the plane the last pixel→world used.
    pub fn work_plane_normal(&self) -> Option<DVec3> {
        self.inference.last_work_plane.map(|p| p.1)
    }

    /// Ray/plane hit, or `None` behind the camera or grazing.
    pub fn ray_plane(
        &self,
        origin: DVec3,
        direction: DVec3,
        point: DVec3,
        normal: DVec3,
    ) -> Option<DVec3> {
        let len = normal.length();
        if len < 1e-9 {
            return None;
        }
        let n = normal / len;
        let denom = n.dot(direction);
        if denom.abs() < GRAZING_PLANE_DEG.to_radians().sin() {
            return None;
        }
        let t = n.dot(point - origin) / denom;
        if t < 0.0 {
            return None;
        }
        Some(origin + direction * t)
    }

    /// `(point, normal)` of the current drawing plane: the tool's drag
    /// plane > its captured plane > the camera-aware plane through its
    /// start point > the face under the cursor > the plane facing an
    /// axis-aligned view > the ground.
    pub fn current_work_plane(&mut self, cursor: Option<(f64, f64)>) -> Plane {
        if let Some(own) = self.active_tool().and_then(|t| t.drag_plane(self)) {
            return own;
        }
        let has_tool = self.active_tool().is_some();
        let captured = self
            .active_tool()
            .and_then(|t| t.work_plane())
            .and_then(|w| w.plane);
        let start = self.tool_start_point();
        if let Some(captured) = captured {
            let tool = self.active_tool();
            let plane_lock = tool
                .and_then(|t| t.work_plane())
                .is_some_and(|w| w.lock.is_some());
            if let Some(s) = start {
                if captured.1.unit_or_zero().z.abs() > 0.94 && !plane_lock {
                    if let Some(v) = self.near_horizon_vertical(s) {
                        return v;
                    }
                }
            }
            return captured;
        }
        let Some(start) = start else {
            if let (Some((x, y)), true) = (cursor, has_tool) {
                if let Some(hit) = self.pick_face_placement(x, y) {
                    return self.face_plane_world(&hit);
                }
            }
            let forward = self.camera.target - self.camera.eye();
            let center = match self.scene.bounds() {
                Some((lo, hi)) => (lo + hi) * 0.5,
                None => DVec3::ZERO,
            };
            if let Some(p) = snap::first_point_work_plane(forward, center, 0.97) {
                return p;
            }
            return (DVec3::ZERO, DVec3::Z);
        };
        let base = self.start_point_plane(start);
        if let (Some(c), None) = (cursor, self.inference.axis_lock) {
            if let Some(fp) = self.hover_face_plane(c) {
                let cand = self.ray_hit_plane(c, base);
                let angle = self
                    .active_tool()
                    .and_then(|t| t.snap_prefs().magnetic_axis_deg)
                    .unwrap_or(self.prefs.inference_angle_deg);
                if cand.is_none_or(|p| snap::detect_axis_alignment(start, p, angle).is_none()) {
                    return fp;
                }
            }
        }
        base
    }

    /// World plane of an unselected face under the cursor.
    pub fn hover_face_plane(&mut self, cursor: (f64, f64)) -> Option<Plane> {
        let hit = self.pick_face_placement(cursor.0, cursor.1)?;
        let sel = &self.scene.selection;
        match hit.owner {
            Some(owner) => {
                if sel.contains(&EntityRef::Group(owner)) {
                    return None;
                }
            }
            None => {
                if sel.contains(&hit.entity()) {
                    return None;
                }
            }
        }
        Some(self.face_plane_world(&hit))
    }

    pub fn ray_hit_plane(&self, cursor: (f64, f64), plane: Plane) -> Option<DVec3> {
        let (o, d) = self.pixel_to_ray(ScreenPt::new(cursor.0, cursor.1))?;
        let (point, normal) = plane;
        let denom = normal.dot(d);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = normal.dot(point - o) / denom;
        (t >= 0.0).then(|| o + d * t)
    }

    /// The camera-aware plane through the active start point.
    pub fn start_point_plane(&self, start: DVec3) -> Plane {
        let forward = self.camera.target - self.camera.eye();
        if forward.length() < 1e-9 {
            return (start, DVec3::Z);
        }
        let forward = forward.unit_or_zero();
        if self
            .active_tool()
            .is_some_and(|t| t.snap_prefs().prefers_vertical_drag)
        {
            let horiz = DVec3::new(forward.x, forward.y, 0.0);
            if horiz.length() >= HORIZON_PITCH_THRESHOLD_DEG.to_radians().sin() {
                return (start, horiz.unit_or_zero());
            }
            return (start, DVec3::Z);
        }
        self.near_horizon_vertical(start)
            .unwrap_or((start, DVec3::Z))
    }

    /// The vertical work plane through `start` when the camera sits near
    /// the horizon, else `None`.
    pub fn near_horizon_vertical(&self, start: DVec3) -> Option<Plane> {
        let forward = self.camera.target - self.camera.eye();
        if forward.length() < 1e-9 {
            return None;
        }
        let forward = forward.unit_or_zero();
        if forward.z.abs() >= HORIZON_PITCH_THRESHOLD_DEG.to_radians().sin() {
            return None;
        }
        Some(if forward.x.abs() >= forward.y.abs() {
            (start, DVec3::X)
        } else {
            (start, DVec3::Y)
        })
    }

    /// Closest point on the lock line (`start`, `lock_dir`) to the camera
    /// ray through the pixel.
    pub fn project_to_lock_line(&self, start: DVec3, lock_dir: DVec3, px: f64, py: f64) -> DVec3 {
        self.probe
            .project_to_lock_line(self, start, lock_dir, ScreenPt::new(px, py))
    }

    /// A world segment → its two screen pixels, cut where it passes
    /// behind the eye and to the window (with a margin).
    pub fn segment_to_pixels(&self, p0: DVec3, p1: DVec3) -> Option<((f64, f64), (f64, f64))> {
        let mvp = self.mvp();
        let mut c0 = mvp * p0.extend(1.0);
        let mut c1 = mvp * p1.extend(1.0);
        let (w0, w1, eps) = (c0.w, c1.w, CLIP_W);
        if w0 < eps && w1 < eps {
            return None;
        }
        if w0 < eps {
            c0 = c0 + (c1 - c0) * ((eps - w0) / (w1 - w0));
            c0.w = eps;
        } else if w1 < eps {
            c1 = c1 + (c0 - c1) * ((eps - w1) / (w0 - w1));
            c1.w = eps;
        }
        let px = |c: DVec4| {
            (
                (c.x / c.w * 0.5 + 0.5) * self.width(),
                (1.0 - (c.y / c.w * 0.5 + 0.5)) * self.height(),
            )
        };
        let ((x0, y0), (x1, y1)) = (px(c0), px(c1));
        let m = 64.0;
        let (lo_x, hi_x, lo_y, hi_y) = (-m, self.width() + m, -m, self.height() + m);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (p, q) in [
            (-dx, x0 - lo_x),
            (dx, hi_x - x0),
            (-dy, y0 - lo_y),
            (dy, hi_y - y0),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
        if t0 > t1 {
            return None;
        }
        Some(((x0 + dx * t0, y0 + dy * t0), (x0 + dx * t1, y0 + dy * t1)))
    }

    /// The span of guide line `g` the snap engine works on: the part
    /// inside the view (widened by `margin`), in double precision.
    pub fn guide_snap_segment(&self, g: &Guide, margin: f64) -> Option<(DVec3, DVec3)> {
        let u = g.direction?;
        let p = g.point;
        let mvp = self.mvp();
        let c0 = mvp * p.extend(1.0);
        let c1 = mvp * u.extend(0.0);
        let (c0, c1) = ((c0.x, c0.y, c0.w), (c1.x, c1.y, c1.w));
        let (mut lo, mut hi) = (-GUIDE_HALF_LEN, GUIDE_HALF_LEN);
        let planes = [
            (c0.2 - 1e-3, c1.2),
            (margin * c0.2 - c0.0, margin * c1.2 - c1.0),
            (margin * c0.2 + c0.0, margin * c1.2 + c1.0),
            (margin * c0.2 - c0.1, margin * c1.2 - c1.1),
            (margin * c0.2 + c0.1, margin * c1.2 + c1.1),
        ];
        for (f0, f1) in planes {
            if f1.abs() < 1e-15 {
                if f0 < 0.0 {
                    return None;
                }
                continue;
            }
            let s = -f0 / f1;
            if f1 > 0.0 {
                lo = lo.max(s);
            } else {
                hi = hi.min(s);
            }
            if lo >= hi {
                return None;
            }
        }
        Some((p + u * lo, p + u * hi))
    }
}
