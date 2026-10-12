// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Protractor tool and the protractor mechanics Rotate shares.
//!
//! Before the first click the disc follows the cursor, aligned to the face
//! under it (arrows lock the plane, Shift freezes it); it keeps a fixed
//! screen size with ticks every 15°; near the disc the angle snaps to the
//! ticks, farther out it is free at 0.1°. A click-drag from the centre
//! tilts the instrument along the drag.

use std::f64::consts::PI;
use xtrude_core::Axis;

use glam::DVec3;
use xtrude_core::axes::{self, plane_axes};
use xtrude_core::math::{round_even, round_even_int, VecExt};
use xtrude_model::commands::{AddGuideCommand, ChangeGuideCommand};
use xtrude_model::guide::Guide;
use xtrude_model::i18n::tr;
use xtrude_model::ids::ObjId;

use super::{
    Plane, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane, DISC_PX, MAGNET_DEG,
    TICK_DEG,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::{Cursor, Viewport};

/// The disc colour off the axis planes.
pub const OFF_AXIS_RGBA: Rgba = [0.24, 0.27, 0.32, 1.0];
/// How far (px) the cursor must travel from the centre for a press to tilt
/// the instrument.
const AXIS_DRAG_PX: f64 = 8.0;
/// Snap kinds that are not a precise target.
const FREE_SNAPS: [&str; 2] = ["none", "on_face"];

/// The shared protractor state.
#[derive(Clone, Debug)]
pub struct ProtractorBase {
    pub start_point: Option<DVec3>,
    pub ref_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub axis_pick: Option<Axis>,
    pub shift_normal: Option<DVec3>,
    pub custom_axis: Option<DVec3>,
    pub axis_drag_armed: bool,
    pub axis_drag_live: Option<DVec3>,
    pub disc_r: f64,
    pub snap_ticks: bool,
    pub exact_snap: bool,
}

impl Default for ProtractorBase {
    fn default() -> Self {
        ProtractorBase {
            start_point: None,
            ref_point: None,
            hover_point: None,
            work_plane: None,
            axis_pick: None,
            shift_normal: None,
            custom_axis: None,
            axis_drag_armed: false,
            axis_drag_live: None,
            disc_r: 1.0,
            snap_ticks: false,
            exact_snap: false,
        }
    }
}

impl ProtractorBase {
    /// While the centre is held, the axis the drag WOULD set (live tilt).
    pub fn track_axis_drag(&mut self, vp: &Viewport) {
        if !self.axis_drag_armed {
            self.axis_drag_live = None;
            return;
        }
        let (Some(s), Some(h)) = (self.start_point, self.hover_point) else {
            self.axis_drag_live = None;
            return;
        };
        if vp.probe().projects() {
            match (vp.world_to_pixel(s), vp.world_to_pixel(h)) {
                (Some(p0), Some(p1)) if (p1.x - p0.x).hypot(p1.y - p0.y) > AXIS_DRAG_PX => {}
                _ => {
                    self.axis_drag_live = None;
                    return;
                }
            }
        }
        let d = h - s;
        self.axis_drag_live = (d.length() > 1e-9).then(|| d.unit_or_zero());
    }

    /// A real drag from the centre fixes the axis along it; true when set.
    pub fn release_axis_drag(&mut self, vp: &mut Viewport) -> bool {
        if !self.axis_drag_armed {
            return false;
        }
        self.track_axis_drag(vp);
        self.axis_drag_armed = false;
        let axis = self.axis_drag_live.take();
        let Some(axis) = axis else { return false };
        self.custom_axis = Some(axis);
        vp.update();
        true
    }

    /// Arrow keys lock the plane to an axis; the same arrow releases it.
    pub fn arrow_key(&mut self, vp: &mut Viewport, key: Key) -> bool {
        let axis = match key {
            Key::Right => Axis::X,
            Key::Left => Axis::Y,
            Key::Up => Axis::Z,
            _ => return false,
        };
        self.axis_pick = if self.axis_pick == Some(axis) {
            None
        } else {
            Some(axis)
        };
        if let (Some(a), None) = (self.axis_pick, self.start_point) {
            let anchor = self.hover_point.unwrap_or(DVec3::ZERO);
            self.work_plane = Some((anchor, axes::axis(a)));
        }
        vp.update();
        true
    }

    /// The disc's colour: the axis it rotates about, dark off the axes.
    pub fn wireframe_color(&self) -> Rgba {
        let n = self.axis();
        for a in Axis::ALL {
            if n.dot(axes::axis(a)).abs() > 0.999 {
                let [r, g, b] = a.color();
                return [r, g, b, 1.0];
            }
        }
        OFF_AXIS_RGBA
    }

    pub fn axis(&self) -> DVec3 {
        if let Some(a) = self.custom_axis {
            return a;
        }
        if let Some(n) = self.shift_normal {
            return n;
        }
        if let Some(a) = self.axis_pick {
            return axes::axis(a);
        }
        if let Some(p) = self.work_plane {
            return p.1.unit_or_zero();
        }
        DVec3::Z
    }

    /// Plane inference until the centre is placed.
    pub fn infer_plane(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let shift = ctx.modifiers.shift;
        if !shift {
            self.shift_normal = None;
        }
        if self.start_point.is_some() {
            return;
        }
        if shift {
            if self.shift_normal.is_none() {
                self.shift_normal = Some(self.axis());
            }
            return;
        }
        if let Some(a) = self.axis_pick {
            self.work_plane = Some((ctx.world, axes::axis(a)));
            return;
        }
        match vp.pick_face_any(ctx.screen.0, ctx.screen.1) {
            Some(hit) => {
                let n = vp
                    .scene
                    .doc
                    .mesh(hit.mesh)
                    .face_normal(hit.face)
                    .unit_or_zero();
                self.work_plane = Some((ctx.world, n));
            }
            None => self.work_plane = None,
        }
    }

    /// Fixed screen-size disc and the distance rule.
    pub fn update_screen_metrics(&mut self, vp: &Viewport, ctx: &ToolContext) {
        let Some(centre) = self.start_point.or(self.hover_point) else {
            return;
        };
        let (u, _v) = plane_axes(self.axis());
        let (Some(p0), Some(p1)) = (vp.world_to_pixel(centre), vp.world_to_pixel(centre + u))
        else {
            return;
        };
        let ppu = (p1.x - p0.x).hypot(p1.y - p0.y);
        if ppu > 1e-6 {
            self.disc_r = DISC_PX / ppu;
        }
        self.snap_ticks = self.start_point.is_some()
            && (ctx.screen.0 - p0.x).hypot(ctx.screen.1 - p0.y) <= DISC_PX * 1.25;
    }

    /// The signed angle (degrees) from the base arm to `point`.
    pub fn angle_to(&self, point: DVec3) -> Option<f64> {
        let (s, r) = (self.start_point?, self.ref_point?);
        let (u, v) = plane_axes(self.axis());
        let (a, b) = (r - s, point - s);
        let a2 = (a.dot(u), a.dot(v));
        let b2 = (b.dot(u), b.dot(v));
        if a2.0.hypot(a2.1) < 1e-9 || b2.0.hypot(b2.1) < 1e-9 {
            return None;
        }
        let mut deg = (b2.1.atan2(b2.0) - a2.1.atan2(a2.0)).to_degrees();
        while deg <= -180.0 {
            deg += 360.0;
        }
        while deg > 180.0 {
            deg -= 360.0;
        }
        Some(deg)
    }

    /// The angle as reported: 15° ticks near the disc, 0.1° farther out.
    pub fn display_deg(&self, point: DVec3) -> Option<f64> {
        let mut deg = self.angle_to(point)?;
        if self.snap_ticks {
            deg = round_even_int(deg / TICK_DEG) as f64 * TICK_DEG;
            if deg <= -180.0 {
                deg += 360.0;
            }
        }
        Some(round_even(deg, 1))
    }

    pub fn note_snap(&mut self, ctx: &ToolContext) {
        self.exact_snap = !FREE_SNAPS.contains(&ctx.snap_kind());
    }

    /// The angle to APPLY: exact on an inferred point.
    pub fn commit_deg(&self, point: DVec3) -> Option<f64> {
        if self.exact_snap && !self.snap_ticks {
            return self.angle_to(point);
        }
        self.display_deg(point)
    }

    /// The base arm turned by `deg` in the plane.
    pub fn direction_at(&self, deg: f64) -> DVec3 {
        let (u, v) = plane_axes(self.axis());
        let a = self.ref_point.unwrap_or_default() - self.start_point.unwrap_or_default();
        let a0 = a.dot(v).atan2(a.dot(u));
        let t = a0 + deg.to_radians();
        (u * t.cos() + v * t.sin()).unit_or_zero()
    }

    /// The fixed-screen-size disc, its zero on the base arm once set.
    pub fn disc(&self, centre: DVec3, axis: Option<DVec3>) -> Vec<(DVec3, DVec3)> {
        let (u, v) = plane_axes(axis.unwrap_or_else(|| self.axis()));
        let r = self.disc_r;
        let mut base = 0.0;
        if let (Some(s), Some(rp)) = (self.start_point, self.ref_point) {
            let a = rp - s;
            base = a.dot(v).atan2(a.dot(u));
        }
        let rim = |t: f64, k: f64| centre + (u * t.cos() + v * t.sin()) * (r * k);
        let n = 48;
        let pts: Vec<DVec3> = (0..n)
            .map(|k| rim(base + 2.0 * PI * k as f64 / n as f64, 1.0))
            .collect();
        let mut segs: Vec<(DVec3, DVec3)> = (0..n).map(|k| (pts[k], pts[(k + 1) % n])).collect();
        for k in 0..(360.0 / TICK_DEG) as usize {
            let deg = k as f64 * TICK_DEG;
            let t = base + deg.to_radians();
            let inner = if deg % 90.0 == 0.0 { 0.75 } else { 0.86 };
            segs.push((rim(t, inner), rim(t, 1.0)));
        }
        segs
    }

    /// The disc (tilted live while a drag from the centre is armed), plus
    /// the arms.
    pub fn preview(&self) -> Vec<(DVec3, DVec3)> {
        let Some(centre) = self.start_point.or(self.hover_point) else {
            return Vec::new();
        };
        let tilt = if self.axis_drag_armed {
            self.axis_drag_live
        } else {
            None
        };
        let mut segs = self.disc(centre, tilt);
        let (Some(s), Some(h)) = (self.start_point, self.hover_point) else {
            return segs;
        };
        segs.push((s, h));
        if let Some(r) = self.ref_point {
            segs.push((s, r));
        }
        segs
    }

    pub fn reset(&mut self) {
        self.start_point = None;
        self.ref_point = None;
        self.work_plane = None;
        self.shift_normal = None;
        self.custom_axis = None;
        self.axis_drag_live = None;
        self.axis_drag_armed = false;
        self.snap_ticks = false;
        self.exact_snap = false;
    }
}

/// The guide just created, for the hot retype.
#[derive(Clone, Debug)]
struct LastGuide {
    guide: ObjId,
    u: DVec3,
    v: DVec3,
    a0: f64,
    sign: f64,
}

/// The Protractor: angled guide lines (or, with Ctrl, a measured angle).
pub struct ProtractorTool {
    pub p: ProtractorBase,
    /// Leave a guide behind (Ctrl toggles; reset when picked up).
    pub guides: bool,
    last: Option<LastGuide>,
}

impl Default for ProtractorTool {
    fn default() -> Self {
        ProtractorTool::new()
    }
}

impl ProtractorTool {
    pub fn new() -> ProtractorTool {
        ProtractorTool {
            p: ProtractorBase::default(),
            guides: true,
            last: None,
        }
    }

    fn commit(&mut self, vp: &mut Viewport, deg: f64) {
        let d = self.p.direction_at(deg);
        if !self.guides {
            let msg = tr("Angle: {a}°", &[("a", &format!("{deg:+.1}"))]);
            vp.flash_status(&msg, 4000);
            self.p.reset();
            vp.update();
            return;
        }
        let (Some(s), Some(r)) = (self.p.start_point, self.p.ref_point) else {
            return;
        };
        let guide = Guide::new(s, Some(d), None);
        let gid = guide.id;
        vp.run(AddGuideCommand::new(guide));
        let (u, v) = plane_axes(self.p.axis());
        let a = r - s;
        self.last = Some(LastGuide {
            guide: gid,
            u,
            v,
            a0: a.dot(v).atan2(a.dot(u)),
            sign: if deg < 0.0 { -1.0 } else { 1.0 },
        });
        self.p.reset();
        vp.update();
    }

    /// The little + beside the cursor: this one will leave a guide.
    pub fn cursor_plus(&self) -> bool {
        self.guides
    }
}

impl Tool for ProtractorTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("protractor"),
            shortcut: Some("Shift+H"),
            description: Some("Measure angles and place guide lines at an angle."),
            ..ToolInfo::new("protractor", "Protractor")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Angle".into())
    }
    fn cursor(&self, _vp: &Viewport) -> Option<Cursor> {
        self.info().icon.map(|key| Cursor::Tool {
            key,
            plus: self.cursor_plus(),
        })
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some(self.p.wireframe_color())
    }
    fn status_clause(&self) -> String {
        let (guide, measure) = (tr("guide", &[]), tr("measure", &[]));
        if self.guides {
            format!("Ctrl = [{guide}] / {measure}")
        } else {
            format!("Ctrl = {guide} / [{measure}]")
        }
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.guides = true;
        self.p.reset();
        self.last = None;
        self.p.axis_pick = None;
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.p.reset();
        self.last = None;
        self.p.axis_pick = None;
        self.p.hover_point = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.last = None;
        let Some(s) = self.p.start_point else {
            self.p.start_point = Some(ctx.world);
            self.p.axis_drag_armed = true;
            return;
        };
        if self.p.ref_point.is_none() {
            if (ctx.world - s).length() < 1e-6 {
                return;
            }
            self.p.ref_point = Some(ctx.world);
            return;
        }
        self.p.note_snap(ctx);
        if let Some(deg) = self.p.commit_deg(ctx.world) {
            self.commit(vp, deg);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.p.hover_point = Some(ctx.world);
        self.p.track_axis_drag(vp);
        self.p.infer_plane(vp, ctx);
        self.p.update_screen_metrics(vp, ctx);
        self.p.note_snap(ctx);
        vp.update();
    }

    fn on_release(&mut self, vp: &mut Viewport) {
        if self.p.release_axis_drag(vp) {
            vp.flash_status(&tr("Protractor axis set along the drag", &[]), 2500);
        }
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: true,
            angle_ratio: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(value) = value.as_num() else {
            return false;
        };
        if self.p.ref_point.is_some() {
            let mut sign = 1.0;
            if let Some(cur) = self.p.hover_point.and_then(|h| self.p.angle_to(h)) {
                if cur < 0.0 {
                    sign = -1.0;
                }
            }
            self.commit(vp, sign * value.abs());
            return true;
        }
        if let Some(last) = self.last.clone() {
            if !vp.scene.doc.guides.iter().any(|g| g.id == last.guide) {
                self.last = None;
                return false;
            }
            let side = last.sign * if value >= 0.0 { 1.0 } else { -1.0 };
            let t = last.a0 + (side * value.abs()).to_radians();
            let d = last.u * t.cos() + last.v * t.sin();
            vp.run(ChangeGuideCommand {
                id: last.guide,
                direction: d,
            });
            vp.update();
            return true;
        }
        false
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.p.reset();
        self.last = None;
        self.p.axis_pick = None;
        vp.update();
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if key == Key::Control {
            self.guides = !self.guides;
            let msg = tr(
                if self.guides {
                    "Create guides: on"
                } else {
                    "Create guides: off — measure only"
                },
                &[],
            );
            vp.flash_status(&msg, 2500);
            vp.refresh_tool_cursor();
            vp.update();
            return true;
        }
        self.p.arrow_key(vp, key)
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        self.p.preview()
    }

    fn guide_preview_lines(&self) -> Vec<(DVec3, DVec3)> {
        if !self.guides {
            return Vec::new();
        }
        let (Some(s), Some(_r), Some(h)) =
            (self.p.start_point, self.p.ref_point, self.p.hover_point)
        else {
            return Vec::new();
        };
        let Some(deg) = self.p.commit_deg(h) else {
            return Vec::new();
        };
        vec![Guide::new(s, Some(self.p.direction_at(deg)), None).segment()]
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        self.p.ref_point?;
        let h = self.p.hover_point?;
        let deg = self.p.display_deg(h)?;
        Some((format!("{deg:+.1}°"), Some(h)))
    }

    fn start_point(&self) -> Option<DVec3> {
        self.p.start_point
    }
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        Some(WorkPlane {
            plane: self.p.work_plane,
            lock: None,
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.p.work_plane = plane;
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            magnetic_axis_deg: Some(MAGNET_DEG),
            radial_arm: true,
            ..SnapPrefs::default()
        }
    }
}
