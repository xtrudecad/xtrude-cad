// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotate tool: turn geometry with the protractor.
//!
//! Centre click (a drag from it sets a custom axis), the reference arm,
//! then the sweep: the geometry swings live and a third click commits.
//! Ctrl rotates a copy; after a commit the angle stays hot (typed values
//! redo it) and after a copy "3x" / "/3" lay a polar array.

use crate::value_box::{ArrayKind, ArraySpec, ValueSpec};
use glam::{DMat4, DVec3};
use xtrude_core::axes::plane_axes;
use xtrude_core::math::{MatExt, VecExt};
use xtrude_core::VertexId;
use xtrude_model::commands::{
    rotation_matrix, RotateGroupCommand, RotateSectionPlanesCommand, RotateVerticesCommand,
};
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId, ObjId};

use super::protractor::ProtractorBase;
use super::transform::{
    gather_copy_entities, gather_targets, selected_section_planes, CmdMark, EdgeSnap, FaceSnap,
    RotateCopies,
};
use super::{
    Plane, Rgba, SnapExcluded, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane,
    MAGNET_DEG,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::Viewport;

/// The preview's snapshot of the rotating set: every frame is
/// computed from it, and reverting puts these exact values back.
#[derive(Clone, Debug, Default)]
struct Snapshot {
    xf: Vec<(GroupId, DMat4)>,
    gv: Vec<(MeshId, VertexId, DVec3)>,
    v: Vec<(VertexId, DVec3)>,
    sp: Vec<(ObjId, DVec3, DVec3)>,
}

/// What a hot retype / polar array redoes.
#[derive(Clone, Debug)]
struct LastRotation {
    mark: CmdMark,
    deg: f64,
    copy: bool,
    sign: f64,
    start: DVec3,
    axis: DVec3,
    groups: Vec<GroupId>,
    splanes: Vec<ObjId>,
    positions: Vec<DVec3>,
    faces: Vec<FaceSnap>,
    edges: Vec<EdgeSnap>,
}

impl LastRotation {
    /// The command for `degs` (one angle, or a copy fan).
    fn build(&self, degs: &[f64]) -> Option<Box<dyn Command>> {
        if self.copy {
            if self.groups.is_empty() && self.faces.is_empty() && self.edges.is_empty() {
                return None;
            }
            return Some(Box::new(RotateCopies {
                groups: self.groups.clone(),
                faces: self.faces.clone(),
                edges: self.edges.clone(),
                center: self.start,
                axis: self.axis,
                degs: degs.to_vec(),
            }));
        }
        let deg = *degs.last()?;
        let mut cmds: Vec<Box<dyn Command>> = self
            .groups
            .iter()
            .map(|&g| {
                Box::new(RotateGroupCommand {
                    group: g,
                    center: self.start,
                    axis: self.axis,
                    degrees: deg,
                }) as Box<dyn Command>
            })
            .collect();
        if !self.positions.is_empty() {
            cmds.push(Box::new(RotateVerticesCommand {
                src: self.positions.clone(),
                center: self.start,
                axis: self.axis,
                degrees: deg,
            }));
        }
        if !self.splanes.is_empty() {
            cmds.push(Box::new(RotateSectionPlanesCommand {
                ids: self.splanes.clone(),
                centre: self.start,
                axis: self.axis,
                deg,
            }));
        }
        match cmds.len() {
            0 => None,
            1 => cmds.pop(),
            _ => Some(Box::new(CompoundCommand::new(cmds))),
        }
    }
}

#[derive(Default)]
pub struct RotateTool {
    pub p: ProtractorBase,
    pub groups: Vec<GroupId>,
    pub splanes: Vec<ObjId>,
    pub positions: Vec<DVec3>,
    pub verts: Vec<VertexId>,
    pub sel_faces: Vec<FaceSnap>,
    pub sel_edges: Vec<EdgeSnap>,
    pub base_segments: Vec<(DVec3, DVec3)>,
    pub preview_deg: f64,
    orig: Option<Snapshot>,
    /// Ctrl: rotate a COPY.
    pub copy: bool,
    last: Option<LastRotation>,
    /// Groups-only rotate: the viewport previews, nothing is mutated.
    pub vp_preview: bool,
}

impl RotateTool {
    pub fn new() -> RotateTool {
        RotateTool::default()
    }

    fn reset(&mut self) {
        self.p.reset();
        self.groups.clear();
        self.splanes.clear();
        self.positions.clear();
        self.verts.clear();
        self.sel_faces.clear();
        self.sel_edges.clear();
        self.base_segments.clear();
        self.preview_deg = 0.0;
        self.orig = None;
        self.copy = false;
    }

    /// Set up the copy entities (Move's grips hand the turn over here).
    pub fn gather_copy_entities(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (f, e, s) = gather_copy_entities(vp, ctx, &self.groups);
        self.sel_faces = f;
        self.sel_edges = e;
        self.base_segments = s;
    }

    fn snapshot(&mut self, vp: &Viewport) -> Snapshot {
        if let Some(s) = &self.orig {
            return s.clone();
        }
        let mut snap = Snapshot::default();
        if !self.vp_preview {
            for &gid in &self.groups {
                let Some(g) = vp.scene.doc.find_group(gid) else {
                    continue;
                };
                match g.xform {
                    Some(x) => snap.xf.push((gid, x)),
                    None => {
                        let m = vp.scene.doc.mesh(g.mesh);
                        snap.gv
                            .extend(m.verts.iter().map(|(id, v)| (g.mesh, id, v.position)));
                    }
                }
            }
            let mesh = vp.scene.mesh();
            snap.v = self
                .verts
                .iter()
                .filter(|&&v| mesh.has_vertex(v))
                .map(|&v| (v, mesh.pos(v)))
                .collect();
        }
        snap.sp = self
            .splanes
            .iter()
            .filter_map(|id| vp.scene.doc.section_planes.iter().find(|p| p.id == *id))
            .map(|p| (p.id, p.point, p.normal))
            .collect();
        self.orig = Some(snap.clone());
        snap
    }

    /// Place the rotating set at `m` applied to its snapshot (`None`: back
    /// as it was).
    fn pose(&mut self, vp: &mut Viewport, m: Option<DMat4>) {
        let snap = self.snapshot(vp);
        for (gid, xf) in &snap.xf {
            if let Some(g) = vp.scene.doc.find_group_mut(*gid) {
                g.xform = Some(m.map_or(*xf, |m| m * *xf));
            }
        }
        for (mid, v, pos) in &snap.gv {
            let mesh = vp.scene.doc.mesh_mut(*mid);
            if mesh.has_vertex(*v) {
                mesh.place_vertex(*v, m.map_or(*pos, |m| m.project_point(*pos)));
            }
        }
        for (v, pos) in &snap.v {
            let mesh = vp.scene.mesh_mut();
            if mesh.has_vertex(*v) {
                mesh.place_vertex(*v, m.map_or(*pos, |m| m.project_point(*pos)));
            }
        }
        for (id, point, normal) in &snap.sp {
            if let Some(sp) = vp.scene.doc.section_planes.iter_mut().find(|p| p.id == *id) {
                sp.point = m.map_or(*point, |m| m.project_point(*point));
                let n2 = m.map_or(*normal, |m| m.transform_vector(*normal));
                sp.normal = if n2.length() > 1e-12 {
                    n2.unit_or_zero()
                } else {
                    *normal
                };
            }
        }
        if !(snap.xf.is_empty() && snap.gv.is_empty() && snap.v.is_empty()) {
            vp.scene.version += 1;
        }
    }

    fn apply_preview(&mut self, vp: &mut Viewport, deg: f64) {
        let Some(s) = self.p.start_point else { return };
        let m = rotation_matrix(s, self.p.axis(), deg);
        self.pose(vp, Some(m));
        self.preview_deg = deg;
        if self.vp_preview {
            vp.set_groups_preview_matrix(m);
        }
    }

    fn revert_preview(&mut self, vp: &mut Viewport) {
        if self.orig.is_some() {
            self.pose(vp, None);
            self.orig = None;
        }
        self.preview_deg = 0.0;
        if self.vp_preview {
            vp.set_groups_preview_matrix(DMat4::IDENTITY);
        }
    }

    fn end_vp_preview(&mut self, vp: &mut Viewport) {
        if self.vp_preview {
            vp.end_groups_preview();
        }
        self.vp_preview = false;
    }

    fn make_last(&self) -> LastRotation {
        LastRotation {
            mark: CmdMark::sentinel(),
            deg: 0.0,
            copy: self.copy,
            sign: 1.0,
            start: self.p.start_point.unwrap_or_default(),
            axis: self.p.axis(),
            groups: self.groups.clone(),
            splanes: self.splanes.clone(),
            positions: self.positions.clone(),
            faces: self.sel_faces.clone(),
            edges: self.sel_edges.clone(),
        }
    }

    pub fn commit(&mut self, vp: &mut Viewport, deg: f64) {
        self.revert_preview(vp);
        self.end_vp_preview(vp);
        if deg.abs() > 1e-9 {
            let mut last = self.make_last();
            if let Some(mut cmd) = last.build(&[deg]) {
                vp.execute(cmd.as_mut());
                last.mark = CmdMark::now(vp);
                last.deg = deg;
                last.sign = if deg < 0.0 { -1.0 } else { 1.0 };
                self.last = Some(last);
            }
        }
        self.reset();
        vp.update();
    }

    fn arc_segments(&self, deg: f64) -> Vec<(DVec3, DVec3)> {
        let (Some(s), Some(r)) = (self.p.start_point, self.p.ref_point) else {
            return Vec::new();
        };
        let (u, v) = plane_axes(self.p.axis());
        let a = r - s;
        let rad = a.length() * 0.75;
        let a0 = a.dot(v).atan2(a.dot(u));
        let steps = 2.max((deg.abs() / 10.0).floor() as usize + 1);
        let pts: Vec<DVec3> = (0..=steps)
            .map(|k| {
                let t = a0 + deg.to_radians() * k as f64 / steps as f64;
                s + (u * t.cos() + v * t.sin()) * rad
            })
            .collect();
        pts.windows(2).map(|w| (w[0], w[1])).collect()
    }

    /// The polar array after a rotate-copy: "3x" (multiples of the angle)
    /// or "/3" (dividing it).
    pub fn on_array_value(
        &mut self,
        vp: &mut Viewport,
        count: u32,
        kind: ArrayKind,
        _step: Option<f64>,
    ) -> bool {
        let Some(mut last) = self.last.clone() else {
            return false;
        };
        if !last.copy || self.p.ref_point.is_some() || count < 1 {
            return false;
        }
        if !last.mark.is_last(vp) {
            self.last = None;
            return false;
        }
        let n = count as f64;
        let degs: Vec<f64> = (1..=count)
            .map(|k| {
                if kind == ArrayKind::Internal {
                    last.deg * (k as f64 / n)
                } else {
                    last.deg * k as f64
                }
            })
            .collect();
        vp.undo();
        let Some(mut cmd) = last.build(&degs) else {
            self.last = None;
            return false;
        };
        vp.execute(cmd.as_mut());
        last.mark = CmdMark::now(vp);
        self.last = Some(last);
        vp.flash_status(&tr("{n} copies", &[("n", &count)]), 2500);
        vp.update();
        true
    }
}

impl Tool for RotateTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("rotate"),
            shortcut: Some("Q"),
            description: Some("Turn the selection around a centre with the protractor; Ctrl leaves a copy behind."),
            ..ToolInfo::new("rotate", "Rotate")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Angle".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some(self.p.wireframe_color())
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.copy = false;
        self.last = None;
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        self.end_vp_preview(vp);
        self.reset();
        self.copy = false;
        self.last = None;
        self.p.axis_pick = None;
        self.p.hover_point = None;
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if key == Key::Control {
            self.copy = !self.copy;
            if self.copy {
                self.revert_preview(vp);
                vp.flash_status(&tr("Rotate a copy: on", &[]), 2500);
            } else {
                vp.flash_status(&tr("Rotate a copy: off", &[]), 2500);
            }
            vp.update();
            return true;
        }
        self.p.arrow_key(vp, key)
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.last = None;
        let Some(s) = self.p.start_point else {
            let (mut groups, mut positions) = gather_targets(vp, ctx);
            let mut splanes = selected_section_planes(vp);
            if splanes.is_empty() && vp.scene.selection.is_empty() {
                if let Some(sp) = vp.pick_section_plane(ctx.screen.0, ctx.screen.1) {
                    splanes = vec![sp];
                    groups.clear();
                    positions.clear();
                }
            }
            if groups.is_empty() && positions.is_empty() && splanes.is_empty() {
                vp.flash_status(
                    &tr("Select (or click) the geometry to rotate first", &[]),
                    2500,
                );
                return;
            }
            let mesh = vp.scene.mesh();
            self.verts = positions
                .iter()
                .filter_map(|&p| mesh.vertex_at(p))
                .collect();
            self.groups = groups;
            self.splanes = splanes;
            self.positions = positions;
            self.vp_preview = false;
            if !self.groups.is_empty() && self.verts.is_empty() && self.positions.is_empty() {
                vp.begin_groups_preview(&self.groups.clone());
                self.vp_preview = true;
            }
            self.gather_copy_entities(vp, ctx);
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
        if self.p.ref_point.is_some() && !self.copy {
            if let Some(deg) = self.p.commit_deg(ctx.world) {
                self.apply_preview(vp, deg);
            }
        }
        vp.update();
    }

    fn on_release(&mut self, vp: &mut Viewport) {
        if self.p.release_axis_drag(vp) {
            vp.flash_status(&tr("Rotation axis set along the drag", &[]), 2500);
        }
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: true,
            angle_ratio: true,
            array: Some(ArraySpec { takes_step: false }),
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if let ToolValue::Array { count, kind, step } = *value {
            return self.on_array_value(vp, count, kind, step);
        }
        let Some(value) = value.as_num() else {
            return false;
        };
        if self.p.ref_point.is_some() {
            let mut sign = if self.preview_deg < 0.0 { -1.0 } else { 1.0 };
            if self.copy {
                if let Some(h) = self.p.hover_point {
                    let cur = self.p.angle_to(h);
                    sign = if cur.is_some_and(|c| c < 0.0) {
                        -1.0
                    } else {
                        1.0
                    };
                }
            }
            self.commit(vp, sign * value.abs());
            return true;
        }
        if let Some(mut last) = self.last.clone() {
            if !last.mark.is_last(vp) {
                self.last = None;
                return false;
            }
            let side = last.sign * if value >= 0.0 { 1.0 } else { -1.0 };
            let deg = side * value.abs();
            vp.undo();
            let Some(mut cmd) = last.build(&[deg]) else {
                self.last = None;
                return false;
            };
            vp.execute(cmd.as_mut());
            last.mark = CmdMark::now(vp);
            last.deg = deg;
            self.last = Some(last);
            vp.update();
            return true;
        }
        false
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        self.end_vp_preview(vp);
        self.reset();
        self.last = None;
        vp.update();
    }

    fn snap_excluded(&self, vp: &Viewport) -> Option<SnapExcluded> {
        if self.p.ref_point.is_none() || self.copy {
            return None;
        }
        SnapExcluded::of(vp, &self.verts, &self.groups)
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let mut segs = self.p.preview();
        let (Some(s), Some(h), Some(_r)) =
            (self.p.start_point, self.p.hover_point, self.p.ref_point)
        else {
            return segs;
        };
        if let Some(deg) = self.p.commit_deg(h) {
            segs.extend(self.arc_segments(deg));
            if self.copy && !self.base_segments.is_empty() {
                let m = rotation_matrix(s, self.p.axis(), deg);
                segs.extend(
                    self.base_segments
                        .iter()
                        .map(|(a, b)| (m.project_point(*a), m.project_point(*b))),
                );
            }
        }
        segs
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
