// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Circle and Polygon tools: a centre click, then a radius. Both draw a
//! regular N-gon on the work plane; one vertex points toward the cursor.

use std::f64::consts::PI;

use glam::DVec3;
use xtrude_core::arrangement::coplanar_plane;
use xtrude_core::axes::plane_axes;
use xtrude_model::commands::{
    AddFaceCommand, RebuildPlanarFacesCommand, RebuildPlaneFacesCommand, TagCurveCommand,
};
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::Command;
use xtrude_model::i18n::tr;
use xtrude_model::scene::Scene;
use xtrude_model::units::fmt_len;

use super::{
    world_per_pixel, Plane, PlaneLock, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue,
    WorkPlane, PREVIEW_PX,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

/// Whether the whole drawing (mesh + the new loop) shares one plane — the
/// gate for the planar arrangement.
pub fn flat_drawing(scene: &Scene, new_points: &[DVec3]) -> bool {
    let mesh = scene.mesh();
    let mut verts: Vec<DVec3> = mesh.verts.iter().map(|(_, v)| v.position).collect();
    verts.extend_from_slice(new_points);
    coplanar_plane(&verts, 1e-4).is_some()
}

/// The plane of the new loop when it already carries mesh content — the
/// gate for the scoped per-plane arrangement in 3D scenes.
pub fn busy_plane(scene: &Scene, new_points: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let (origin, normal) = coplanar_plane(new_points, 1e-4)?;
    let tol = 1e-4;
    let mesh = scene.mesh();
    let busy = mesh.edge_ids().into_iter().any(|e| {
        let (a, b) = mesh.edge_ab(e);
        (a - origin).dot(normal).abs() < tol && (b - origin).dot(normal).abs() < tol
    });
    busy.then_some((origin, normal))
}

/// Commit a closed curve loop (circle, polygon): its edges, its face and
/// its curve tag, through the planar arrangement where the plane needs it.
pub fn commit_curve_loop(vp: &mut Viewport, pts: &[DVec3]) {
    commit_loop(vp, pts, &[(pts.to_vec(), true)]);
}

/// Commit a closed loop: its edges, its face, and the runs of it that are
/// curves (`(points, closed)`, a slot's two arcs), through the planar
/// arrangement where the plane needs it.
pub fn commit_loop(vp: &mut Viewport, pts: &[DVec3], curves: &[(Vec<DVec3>, bool)]) {
    let n = pts.len();
    let segs: Vec<(DVec3, DVec3)> = (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect();
    let tags = || -> Vec<Box<dyn Command>> {
        curves
            .iter()
            .map(|(c, closed)| -> Box<dyn Command> {
                Box::new(TagCurveCommand {
                    pts: c.clone(),
                    closed: *closed,
                })
            })
            .collect()
    };
    let mut extra: Vec<Box<dyn Command>> = Vec::new();
    if flat_drawing(&vp.scene, pts) {
        extra.extend(tags());
        extra.push(Box::new(RebuildPlanarFacesCommand::new()));
    } else if let Some((o, nrm)) = busy_plane(&vp.scene, pts) {
        extra.push(Box::new(AddFaceCommand::new(pts.to_vec())));
        extra.extend(tags());
        extra.push(Box::new(RebuildPlaneFacesCommand::new(o, nrm)));
    } else {
        extra.push(Box::new(AddFaceCommand::new(pts.to_vec())));
        extra.extend(tags());
    }
    let cmd = build_add_edges(&vp.scene, &segs, false, extra, None);
    vp.execute_boxed(cmd);
}

/// The shared centre + radius regular-polygon tool.
pub struct RadialTool {
    info: &'static ToolInfo,
    pub sides: usize,
    pub start_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
    wireframe: Option<Rgba>,
}

pub type CircleTool = RadialTool;
pub type PolygonTool = RadialTool;

impl RadialTool {
    /// The Circle: 24 sides.
    pub fn circle() -> RadialTool {
        static CIRCLE: ToolInfo = ToolInfo {
            icon: Some("circle"),
            shortcut: Some("C"),
            description: Some("Draw a circle from its centre and radius."),
            ..ToolInfo::new("circle", "Circle")
        };
        RadialTool::with(&CIRCLE, 24)
    }

    /// The Polygon: 6 sides.
    pub fn polygon() -> RadialTool {
        static POLYGON: ToolInfo = ToolInfo {
            icon: Some("polygon"),
            description: Some(
                "Draw a regular polygon from its centre and radius; type a number followed by «s» to change the sides.",
            ),
            ..ToolInfo::new("polygon", "Polygon")
        };
        RadialTool::with(&POLYGON, 6)
    }

    fn with(info: &'static ToolInfo, sides: usize) -> RadialTool {
        RadialTool {
            info,
            sides,
            start_point: None,
            hover_point: None,
            work_plane: None,
            lock: PlaneLock::default(),
            wireframe: None,
        }
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
        self.wireframe = None;
        self.lock.clear_plane_lock();
    }

    /// `(point, normal)` the shape is laid out on (the snapped plane when
    /// a snapped point lies off the captured one).
    pub fn drawing_plane(&self) -> Plane {
        self.lock
            .drawing_plane(self.work_plane, &self.plane_points())
    }

    /// Make the snapped plane the captured one.
    pub fn adopt_snapped_plane(&mut self) {
        let pts = self.plane_points();
        self.lock.adopt_snapped_plane(&mut self.work_plane, &pts);
    }

    fn plane_points(&self) -> Vec<DVec3> {
        match self.start_point {
            None => Vec::new(),
            Some(s) => std::iter::once(s).chain(self.hover_point).collect(),
        }
    }

    pub fn axes(&self) -> (DVec3, DVec3) {
        plane_axes(
            self.lock
                .drawing_plane(self.work_plane, &self.plane_points())
                .1,
        )
    }

    /// The polygon's corners for `center` and a `rim` point.
    pub fn points(&self, center: DVec3, rim: DVec3) -> Vec<DVec3> {
        let (u, v) = self.axes();
        let d = rim - center;
        let (du, dv) = (d.dot(u), d.dot(v));
        let r = du.hypot(dv);
        if r < 1e-6 {
            return Vec::new();
        }
        let a0 = dv.atan2(du);
        (0..self.sides)
            .map(|k| {
                let a = a0 + 2.0 * PI * k as f64 / self.sides as f64;
                center + (u * a.cos() + v * a.sin()) * r
            })
            .collect()
    }

    fn commit(&mut self, vp: &mut Viewport, pts: &[DVec3]) {
        commit_curve_loop(vp, pts);
        self.reset();
        vp.update();
    }

    fn cursor_preview(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(c) = self.hover_point else {
            return Vec::new();
        };
        let (_p, normal) = self.lock.preview_plane(vp, c);
        let (u, v) = plane_axes(normal);
        let Some(scale) = world_per_pixel(vp, c, u) else {
            return Vec::new();
        };
        let r = PREVIEW_PX * scale;
        let n = self.sides.max(3);
        let pts: Vec<DVec3> = (0..n)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / n as f64;
                c + u * (r * a.cos()) + v * (r * a.sin())
            })
            .collect();
        (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect()
    }

    /// "24s": the side count, at any moment.
    pub fn on_segments_value(&mut self, vp: &mut Viewport, n: u32) -> bool {
        if n < 3 {
            return false;
        }
        self.sides = n as usize;
        vp.flash_status(&tr("{n} sides", &[("n", &n)]), 2500);
        vp.update();
        true
    }
}

impl Tool for RadialTool {
    fn info(&self) -> &'static ToolInfo {
        self.info
    }
    fn value_box_caption(&self) -> Option<String> {
        Some(
            if self.start_point.is_some() {
                "Radius"
            } else {
                "Sides"
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
        let Some(start) = self.start_point else {
            self.start_point = Some(ctx.world);
            if self.work_plane.is_none() {
                self.work_plane = self.lock.locked_work_plane(ctx.world);
            }
            return;
        };
        let pts = self.points(start, ctx.world);
        if !pts.is_empty() {
            self.commit(vp, &pts);
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
            unitless: self.start_point.is_none(),
            segments: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if let ToolValue::Segments(n) = *value {
            return self.on_segments_value(vp, n);
        }
        let Some(value) = value.as_num() else {
            return false;
        };
        let Some(start) = self.start_point else {
            let n = xtrude_core::math::round_even_int(value);
            if n >= 3 {
                self.sides = n as usize;
                vp.flash_status(&tr("{n} sides", &[("n", &n)]), 2500);
                return true;
            }
            return false;
        };
        let Some(hover) = self.hover_point else {
            return false;
        };
        if value <= 0.0 {
            return false;
        }
        let (u, v) = self.axes();
        let d = hover - start;
        let ang = d.dot(v).atan2(d.dot(u));
        let rim = start + (u * ang.cos() + v * ang.sin()) * value;
        let pts = self.points(start, rim);
        if !pts.is_empty() {
            self.commit(vp, &pts);
        }
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
        let Some(start) = self.start_point else {
            return self.cursor_preview(vp);
        };
        let pts = self.points(start, hover);
        let n = pts.len();
        (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect()
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return None;
        };
        let (u, v) = self.axes();
        let d = hover - start;
        let r = d.dot(u).hypot(d.dot(v));
        let sides = tr("{n} sides", &[("n", &self.sides)]);
        Some((format!("R {}  ({sides})", fmt_len(r)), Some(hover)))
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        let started = self.start_point.is_some();
        self.lock.plane_lock_key(vp, key, started)
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
        SnapPrefs::axis_magnet(true)
    }
}
