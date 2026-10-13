// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Move tool: drag geometry (and everything connected to it) to a new spot.
//!
//! The selection (or what is under the first click) is grabbed; the real
//! geometry deforms live as the cursor moves; the second click (or a typed
//! distance / `x;y;z`) drops it. Ctrl moves a copy; right after a copy
//! "3x" / "/3" (or "5x10m") lay an array. Groups show rotation grips on
//! their box that hand the turn to a Rotate tool.

use crate::value_box::{ArrayKind, ArraySpec, ValueSpec};
use glam::{DMat4, DVec3};
use xtrude_core::math::VecExt;
use xtrude_core::VertexId;
use xtrude_model::commands::attrs::{shift_dimension, DimMoveMode};
use xtrude_model::commands::{
    MoveDimensionsCommand, MoveGroupCommand, MoveSectionPlanesCommand, MoveTextLabelsCommand,
    MoveVerticesCommand,
};
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, ObjId};
use xtrude_model::units::fmt_len;

use super::rotate::RotateTool;
use super::transform::{
    array_multiples, gather_copy_entities, gather_dimensions, gather_labels, gather_targets,
    selected_section_planes, CmdMark, CopySet, EdgeSnap, FaceSnap,
};
use super::{
    OverlayLayer, Plane, Rgba, SnapExcluded, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::overlay::{qc, Paint, Pen};
use crate::viewport::Viewport;

/// How near (px) the cursor must come to a rotation grip to take it.
pub const GRIP_PX: f64 = 9.0;
/// Grips sit this fraction of the face's size from its centre.
pub const GRIP_SPREAD: f64 = 0.3;
const FLAT: f64 = 1e-6;

/// A rotation grip: `(position, face centre, face normal)`.
pub type Grip = (DVec3, DVec3, DVec3);

/// Rotation grips on a group's box: four on each face turned toward `eye`.
pub fn rotation_grips(obb: ((DVec3, DVec3, DVec3), DVec3, DVec3), eye: DVec3) -> Vec<Grip> {
    let (frame, lo, hi) = obb;
    let axes = [frame.0, frame.1, frame.2];
    let ext = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
    let mid = [
        (lo.x + hi.x) * 0.5,
        (lo.y + hi.y) * 0.5,
        (lo.z + hi.z) * 0.5,
    ];
    let centre = axes[0] * mid[0] + axes[1] * mid[1] + axes[2] * mid[2];
    let mut out = Vec::new();
    for i in 0..3 {
        let others: Vec<usize> = (0..3).filter(|&a| a != i).collect();
        let (j, k) = (others[0], others[1]);
        if ext[j] <= FLAT && ext[k] <= FLAT {
            continue;
        }
        for side in [1.0, -1.0] {
            let normal = axes[i] * side;
            let face_c = centre + normal * (ext[i] * 0.5);
            if normal.dot(eye - face_c) <= 0.0 {
                continue;
            }
            for a in [j, k] {
                if ext[a] <= FLAT {
                    continue;
                }
                for sgn in [1.0, -1.0] {
                    out.push((
                        face_c + axes[a] * (sgn * GRIP_SPREAD * ext[a]),
                        face_c,
                        normal,
                    ));
                }
            }
        }
    }
    out
}

/// The copy just made, for "3x" / "/3".
#[derive(Clone, Debug)]
struct LastCopy {
    mark: CmdMark,
    set: CopySet,
    delta: DVec3,
}

#[derive(Default)]
pub struct MoveTool {
    pub start_point: Option<DVec3>,
    pub grab: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub positions: Vec<DVec3>,
    pub verts: Vec<VertexId>,
    pub groups: Vec<GroupId>,
    pub splanes: Vec<ObjId>,
    pub labels: Vec<ObjId>,
    pub dims: Vec<(ObjId, DimMoveMode)>,
    pub preview_delta: DVec3,
    /// Ctrl: move a COPY.
    pub copy: bool,
    pub sel_faces: Vec<FaceSnap>,
    pub sel_edges: Vec<EdgeSnap>,
    pub base_segments: Vec<(DVec3, DVec3)>,
    last: Option<LastCopy>,
    pub grip_group: Option<GroupId>,
    pub grips: Vec<Grip>,
    pub hot_grip: Option<usize>,
    pub grip_rot: Option<Box<RotateTool>>,
    grip_rot_done: Option<Box<RotateTool>>,
    pub vp_preview: bool,
}

impl MoveTool {
    pub fn new() -> MoveTool {
        MoveTool::default()
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.grab = None;
        self.hover_point = None;
        self.positions.clear();
        self.verts.clear();
        self.groups.clear();
        self.labels.clear();
        self.dims.clear();
        self.splanes.clear();
        self.preview_delta = DVec3::ZERO;
        self.sel_faces.clear();
        self.sel_edges.clear();
        self.base_segments.clear();
        self.vp_preview = false;
        self.copy = false;
    }

    fn clear_grips(&mut self) {
        self.grip_group = None;
        self.grips.clear();
        self.hot_grip = None;
    }

    fn grip_candidate(vp: &mut Viewport, x: f64, y: f64) -> Option<GroupId> {
        let g = vp.pick_group(x, y)?;
        let sel = &vp.scene.selection;
        if !sel.is_empty()
            && !(sel.len() == 1 && sel.contains(&xtrude_model::entity::EntityRef::Group(g)))
        {
            return None;
        }
        Some(g)
    }

    fn grip_under(&self, vp: &Viewport, x: f64, y: f64) -> Option<usize> {
        let mut best = None;
        let mut best_d = GRIP_PX;
        for (i, (pos, _, _)) in self.grips.iter().enumerate() {
            let Some(p) = vp.world_to_pixel(*pos) else {
                continue;
            };
            let d = (p.x - x).hypot(p.y - y);
            if d <= best_d {
                best = Some(i);
                best_d = d;
            }
        }
        best
    }

    fn track_grips(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let probe = vp.probe();
        if !probe.projects() || probe.eye(vp).is_none() {
            return;
        }
        let (x, y) = ctx.screen;
        let g = Self::grip_candidate(vp, x, y);
        let hot = self.grip_under(vp, x, y);
        if g.is_none() && hot.is_some() {
            self.hot_grip = hot;
            return;
        }
        let eye = vp.eye();
        self.grip_group = g;
        self.grips = match g.and_then(|g| vp.group_obb(g)) {
            Some(obb) => rotation_grips(obb, eye),
            None => Vec::new(),
        };
        self.hot_grip = self.grip_under(vp, x, y);
    }

    fn start_grip_rotation(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (Some(i), Some(group)) = (self.hot_grip, self.grip_group) else {
            return;
        };
        let (pos, face_c, normal) = self.grips[i];
        let mut rot = RotateTool::new();
        rot.on_activate(vp);
        rot.groups = vec![group];
        vp.begin_groups_preview(&[group]);
        rot.vp_preview = true;
        rot.gather_copy_entities(vp, ctx);
        rot.p.custom_axis = Some(normal.unit_or_zero());
        rot.p.start_point = Some(face_c);
        rot.p.ref_point = Some(pos);
        rot.p.hover_point = Some(pos);
        self.grip_rot = Some(Box::new(rot));
        vp.flash_status(
            &tr(
                "Rotate: move to turn, click or type the angle. Ctrl = copy",
                &[],
            ),
            2500,
        );
        vp.update();
    }

    /// Translate the live geometry by `step`.
    fn shift(&self, vp: &mut Viewport, step: DVec3) {
        for &gid in &self.groups {
            let Some(g) = vp.scene.doc.find_group(gid) else {
                continue;
            };
            match g.xform {
                Some(x) => {
                    if let Some(g) = vp.scene.doc.find_group_mut(gid) {
                        g.xform = Some(DMat4::from_translation(step) * x);
                    }
                }
                None => {
                    let mid = g.mesh;
                    let mesh = vp.scene.doc.mesh_mut(mid);
                    for v in mesh.vertex_ids() {
                        mesh.move_vertex(v, step);
                    }
                }
            }
        }
        for &v in &self.verts {
            let mesh = vp.scene.mesh_mut();
            if mesh.has_vertex(v) {
                mesh.move_vertex(v, step);
            }
        }
        self.shift_annotations(vp, step);
        vp.scene.version += 1;
    }

    fn shift_annotations(&self, vp: &mut Viewport, step: DVec3) {
        for lab in vp.scene.doc.text_labels.iter_mut() {
            if self.labels.contains(&lab.id) {
                lab.offset += step;
            }
        }
        let mut dims = std::mem::take(&mut vp.scene.doc.dimensions);
        for d in dims.iter_mut() {
            if let Some(&(_, mode)) = self.dims.iter().find(|(id, _)| *id == d.id) {
                shift_dimension(&vp.scene, d, mode, step);
            }
        }
        vp.scene.doc.dimensions = dims;
        for sp in vp.scene.doc.section_planes.iter_mut() {
            if self.splanes.contains(&sp.id) {
                sp.point += step;
            }
        }
    }

    fn apply_preview(&mut self, vp: &mut Viewport, target: DVec3) {
        let step = target - self.preview_delta;
        if step.length() < 1e-12 {
            return;
        }
        if self.vp_preview {
            self.shift_annotations(vp, step);
            self.preview_delta = target;
            vp.set_groups_preview_offset(target);
            return;
        }
        self.shift(vp, step);
        self.preview_delta = target;
    }

    fn revert_preview(&mut self, vp: &mut Viewport) {
        if self.vp_preview {
            if self.preview_delta.length() >= 1e-12 {
                let step = -self.preview_delta;
                self.shift_annotations(vp, step);
            }
            self.preview_delta = DVec3::ZERO;
            return;
        }
        if self.preview_delta.length() < 1e-12 {
            return;
        }
        let step = -self.preview_delta;
        self.shift(vp, step);
        self.preview_delta = DVec3::ZERO;
    }

    fn commit(&mut self, vp: &mut Viewport, delta: DVec3) {
        vp.end_groups_preview();
        self.revert_preview(vp);
        if self.copy && delta.length() > 1e-9 {
            let mut last = LastCopy {
                mark: CmdMark::sentinel(),
                set: CopySet {
                    groups: self.groups.clone(),
                    faces: self.sel_faces.clone(),
                    edges: self.sel_edges.clone(),
                },
                delta,
            };
            if let Some(mut cmd) = last.set.translated(vec![delta]) {
                vp.execute(cmd.as_mut());
                last.mark = CmdMark::now(vp);
                self.last = Some(last);
            }
            self.reset();
            vp.update();
            return;
        }
        if delta.length() > 1e-9 {
            let mut cmds: Vec<Box<dyn Command>> = self
                .groups
                .iter()
                .map(|&g| Box::new(MoveGroupCommand { group: g, delta }) as Box<dyn Command>)
                .collect();
            if !self.positions.is_empty() {
                cmds.push(Box::new(MoveVerticesCommand {
                    src: self.positions.clone(),
                    delta,
                }));
            }
            if !self.labels.is_empty() {
                cmds.push(Box::new(MoveTextLabelsCommand {
                    ids: self.labels.clone(),
                    delta,
                }));
            }
            if !self.dims.is_empty() {
                cmds.push(Box::new(MoveDimensionsCommand {
                    modes: self.dims.clone(),
                    delta,
                }));
            }
            if !self.splanes.is_empty() {
                cmds.push(Box::new(MoveSectionPlanesCommand {
                    ids: self.splanes.clone(),
                    delta,
                }));
            }
            match cmds.len() {
                0 => {}
                1 => {
                    let mut c = cmds.pop().expect("one command");
                    vp.execute(c.as_mut());
                }
                _ => {
                    vp.execute(&mut CompoundCommand::new(cmds));
                }
            }
        }
        self.reset();
        vp.update();
    }

    /// Arrays after a Move-copy: "3x" external, "/3" internal; "5x10m"
    /// carries the spacing.
    pub fn on_array_value(
        &mut self,
        vp: &mut Viewport,
        count: u32,
        kind: ArrayKind,
        step: Option<f64>,
    ) -> bool {
        if let (Some(step), Some(_)) = (step, self.start_point) {
            let (Some(h), Some(g)) = (self.hover_point, self.grab) else {
                return false;
            };
            if !self.copy {
                return false;
            }
            let d = h - g;
            if d.length() < 1e-9 {
                return false;
            }
            self.commit(vp, d.unit_or_zero() * step);
            if self.last.is_none() {
                return false;
            }
            return self.on_array_value(vp, count, kind, None);
        }
        if let (Some(step), Some(last)) = (step, self.last.as_mut()) {
            if last.delta.length() < 1e-9 {
                return false;
            }
            last.delta = last.delta.unit_or_zero() * step;
        }
        let Some(mut last) = self.last.clone() else {
            return false;
        };
        if self.start_point.is_some() || count < 1 {
            return false;
        }
        if !last.mark.is_last(vp) {
            self.last = None;
            return false;
        }
        let deltas: Vec<DVec3> = array_multiples(count, kind)
            .into_iter()
            .map(|k| last.delta * k)
            .collect();
        vp.undo();
        let Some(mut cmd) = last.set.translated(deltas) else {
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

impl Tool for MoveTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("move"),
            shortcut: Some("M"),
            description: Some(
                "Move the selection, or what you click, to a new place; Ctrl leaves a copy behind.",
            ),
            ..ToolInfo::new("move", "Move")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Distance".into())
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            magnetic_axis_deg: Some(15.0),
            screen_axis_px: Some(9.0),
            prefers_vertical_drag: true,
            ..SnapPrefs::default()
        }
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        self.grip_rot.as_ref().and_then(|r| r.wireframe_color())
    }
    /// The rotation grips, while no grip rotation runs.
    fn overlay(&self, vp: &Viewport, layer: OverlayLayer, out: &mut Vec<Paint>) {
        if layer != OverlayLayer::OverSectionPlanes || self.grip_rot.is_some() {
            return;
        }
        let red = qc(220, 40, 40, 255);
        for (i, (pos, _, _)) in self.grips.iter().enumerate() {
            let Some(p) = vp.world_to_pixel(*pos) else {
                continue;
            };
            let hot = self.hot_grip == Some(i);
            let r = if hot { 6.0 } else { 4.0 };
            let pen = Pen::solid(red, if hot { 3.0 } else { 2.0 });
            out.push(Paint::Line {
                a: (p.x - r, p.y),
                b: (p.x + r, p.y),
                pen,
            });
            out.push(Paint::Line {
                a: (p.x, p.y - r),
                b: (p.x, p.y + r),
                pen,
            });
        }
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.copy = false;
        self.last = None;
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        if let Some(r) = self.grip_rot.as_mut() {
            r.on_cancel(vp);
        }
        self.grip_rot = None;
        self.clear_grips();
        vp.end_groups_preview();
        self.revert_preview(vp);
        self.reset();
        self.copy = false;
        self.last = None;
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, m: Modifiers) -> bool {
        if let Some(r) = self.grip_rot.as_mut() {
            return r.on_key(vp, key, m);
        }
        if key == Key::Control {
            self.copy = !self.copy;
            if self.copy {
                self.revert_preview(vp);
                if self.vp_preview {
                    vp.set_groups_preview_offset(DVec3::ZERO);
                }
                vp.flash_status(&tr("Move a copy: on", &[]), 2500);
            } else {
                if let (Some(g), Some(h)) = (self.grab, self.hover_point) {
                    self.apply_preview(vp, h - g);
                }
                vp.flash_status(&tr("Move a copy: off", &[]), 2500);
            }
            vp.update();
            return true;
        }
        false
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.grip_rot_done = None;
        if let Some(mut rot) = self.grip_rot.take() {
            rot.on_click(vp, ctx);
            if rot.p.start_point.is_none() {
                self.grip_rot_done = Some(rot);
                self.clear_grips();
            } else {
                self.grip_rot = Some(rot);
            }
            return;
        }
        if self.start_point.is_none() && self.hot_grip.is_some() {
            self.start_grip_rotation(vp, ctx);
            return;
        }
        let Some(grab) = self.grab.filter(|_| self.start_point.is_some()) else {
            let (mut groups, mut positions) = gather_targets(vp, ctx);
            let labels = gather_labels(vp, ctx);
            let mut splanes = selected_section_planes(vp);
            let nothing_selected = vp.scene.selection.is_empty();
            if splanes.is_empty() && nothing_selected {
                if let Some(sp) = vp.pick_section_plane(ctx.screen.0, ctx.screen.1) {
                    splanes = vec![sp];
                    groups.clear();
                    positions.clear();
                }
            }
            if !labels.is_empty() && nothing_selected {
                groups.clear();
                positions.clear();
            }
            let mesh = vp.scene.mesh();
            let mut verts: Vec<VertexId> = positions
                .iter()
                .filter_map(|&p| mesh.vertex_at(p))
                .collect();
            let dims = gather_dimensions(vp, ctx, &groups, &verts);
            if !dims.is_empty() && nothing_selected {
                groups.clear();
                positions.clear();
                verts.clear();
            }
            if groups.is_empty()
                && positions.is_empty()
                && labels.is_empty()
                && splanes.is_empty()
                && dims.is_empty()
            {
                return;
            }
            self.last = None;
            self.start_point = Some(ctx.world);
            self.grab = Some(ctx.world);
            self.groups = groups;
            self.positions = positions;
            self.labels = labels;
            self.dims = dims;
            self.splanes = splanes;
            self.verts = verts;
            self.vp_preview = false;
            if !self.groups.is_empty() && self.verts.is_empty() && self.positions.is_empty() {
                vp.begin_groups_preview(&self.groups.clone());
                self.vp_preview = true;
            }
            let (f, e, s) = gather_copy_entities(vp, ctx, &self.groups);
            self.sel_faces = f;
            self.sel_edges = e;
            self.base_segments = s;
            return;
        };
        self.commit(vp, ctx.world - grab);
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if let Some(r) = self.grip_rot.as_mut() {
            r.on_hover(vp, ctx);
            return;
        }
        if self.start_point.is_none() {
            self.track_grips(vp, ctx);
        }
        self.hover_point = Some(ctx.world);
        if let (Some(g), false) = (self.grab, self.copy) {
            self.apply_preview(vp, ctx.world - g);
        }
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: (self.grip_rot.is_some() || self.grip_rot_done.is_some())
                && self.start_point.is_none(),
            array: Some(ArraySpec { takes_step: true }),
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if let ToolValue::Array { count, kind, step } = *value {
            return self.on_array_value(vp, count, kind, step);
        }
        if self.start_point.is_none() {
            let held = self.grip_rot.is_some();
            let rot = if held {
                self.grip_rot.as_mut()
            } else {
                self.grip_rot_done.as_mut()
            };
            if let Some(rot) = rot {
                let done = rot.on_value(vp, value);
                if held && rot.p.start_point.is_none() {
                    self.grip_rot_done = self.grip_rot.take();
                    self.clear_grips();
                }
                return done;
            }
        }
        let (Some(_), Some(grab)) = (self.start_point, self.grab) else {
            return false;
        };
        let delta = match *value {
            ToolValue::Triple(x, y, z) => DVec3::new(x, y, z),
            ToolValue::Num(v) => {
                let Some(h) = self.hover_point else {
                    return false;
                };
                let d = h - grab;
                if d.length() < 1e-9 {
                    return false;
                }
                d.unit_or_zero() * v
            }
            _ => return false,
        };
        self.commit(vp, delta);
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        if let Some(mut r) = self.grip_rot.take() {
            r.on_cancel(vp);
            self.clear_grips();
            vp.update();
            return;
        }
        vp.end_groups_preview();
        self.revert_preview(vp);
        self.reset();
        self.last = None;
        vp.update();
    }

    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        if let Some(r) = &self.grip_rot {
            return r.rubber_band_lines(vp);
        }
        let (Some(g), Some(h)) = (self.grab, self.hover_point) else {
            return Vec::new();
        };
        let mut segs = vec![(g, h)];
        if self.copy && !self.base_segments.is_empty() {
            let d = h - g;
            segs.extend(self.base_segments.iter().map(|(a, b)| (*a + d, *b + d)));
        }
        segs
    }

    fn value_label(&self, vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        if let Some(r) = &self.grip_rot {
            return r.value_label(vp);
        }
        let (g, h) = (self.grab?, self.hover_point?);
        Some((fmt_len((h - g).length()), Some((g + h) * 0.5)))
    }

    fn snap_excluded(&self, vp: &Viewport) -> Option<SnapExcluded> {
        if let Some(r) = &self.grip_rot {
            return r.snap_excluded(vp);
        }
        if self.grab.is_none() || self.copy {
            return None;
        }
        SnapExcluded::of(vp, &self.verts, &self.groups)
    }

    fn drag_plane(&self, _vp: &Viewport) -> Option<Plane> {
        let r = self.grip_rot.as_ref()?;
        Some((r.p.start_point?, r.p.axis()))
    }

    fn start_point(&self) -> Option<DVec3> {
        self.start_point
    }
    fn chain_first_point(&self) -> Option<DVec3> {
        None
    }
    fn busy(&self) -> bool {
        self.start_point.is_some() || self.grab.is_some() || self.grip_rot.is_some()
    }
}
