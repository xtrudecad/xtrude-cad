// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Scale tool — the grip box.
//!
//! The selection sits in a box with grips (26 on a 3D box, 8 on a flat
//! one): corners scale uniformly, edge midpoints two axes, face centres
//! one; the anchor is the opposite side (Ctrl: the centre; Shift: uniform).
//! A plain typed number is a factor, `a;b;c` per axis, a number with a
//! unit the new absolute size; negative mirrors. Right after a commit a
//! typed value redoes it.

use crate::input::ScreenPt;
use glam::{DMat4, DVec3};
use xtrude_core::axes;
use xtrude_core::math::MatExt;
use xtrude_core::Axis;
use xtrude_core::VertexId;
use xtrude_model::commands::{scale_matrix, ScaleFactor, ScaleGroupCommand, ScaleVerticesCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{group_frame, iter_placements};
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::SelectMode;

use super::transform::{gather_targets, CmdMark};
use super::{OverlayLayer, SnapExcluded, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::overlay::{qc, Paint, Pen};
use crate::viewport::Viewport;

const MIN_FACTOR: f64 = 1e-4;
/// Below this extent an axis is flat: no grips, never scales.
const FLAT: f64 = 1e-6;
const AXIS_NAMES: [&str; 3] = ["Red", "Green", "Blue"];
/// Snap kinds a held grip may land on.
const TARGET_KINDS: [&str; 12] = [
    "endpoint",
    "midpoint",
    "intersection",
    "center",
    "origin",
    "on_edge",
    "on_line",
    "from_point",
    "close",
    "guide",
    "component_origin",
    "arc_midpoint",
];

/// One scaling grip: its box-parameter position and the axes it scales.
#[derive(Clone, Debug, PartialEq)]
pub struct Grip {
    /// `(tx, ty, tz)`, each 0 / 0.5 / 1.
    pub params: (f64, f64, f64),
    pub mask: Vec<usize>,
}

impl Grip {
    pub fn kind(&self, active_axes: usize) -> &'static str {
        let n = self.mask.len();
        if n >= active_axes {
            "corner"
        } else if n == 2 {
            "edge"
        } else {
            "face"
        }
    }

    fn t(&self, i: usize) -> f64 {
        match i {
            0 => self.params.0,
            1 => self.params.1,
            _ => self.params.2,
        }
    }
}

/// How a retype reads values: `(uniform, mask, extents, active axes)`.
#[derive(Clone, Debug)]
struct Spec {
    uniform: bool,
    mask: Vec<usize>,
    ext: [f64; 3],
    active: Vec<usize>,
}

#[derive(Clone, Debug)]
struct LastScale {
    mark: CmdMark,
    spec: Spec,
    anchor: DVec3,
    box_axes: Option<(DVec3, DVec3, DVec3)>,
    groups: Vec<GroupId>,
    positions: Vec<DVec3>,
}

impl LastScale {
    fn build(&self, fs: [f64; 3]) -> Option<Box<dyn Command>> {
        let packed = if fs[0] == fs[1] && fs[1] == fs[2] {
            ScaleFactor::Uniform(fs[0])
        } else {
            ScaleFactor::PerAxis(fs)
        };
        let mut cmds: Vec<Box<dyn Command>> = self
            .groups
            .iter()
            .map(|&g| {
                Box::new(ScaleGroupCommand {
                    group: g,
                    center: self.anchor,
                    factor: packed,
                    axes: self.box_axes,
                }) as Box<dyn Command>
            })
            .collect();
        if !self.positions.is_empty() {
            cmds.push(Box::new(ScaleVerticesCommand {
                src: self.positions.clone(),
                center: self.anchor,
                factor: packed,
                axes: self.box_axes,
            }));
        }
        match cmds.len() {
            0 => None,
            1 => cmds.pop(),
            _ => Some(Box::new(CompoundCommand::new(cmds))),
        }
    }
}

/// Everything the scale overlay draws ([`ScaleTool::scale_box_state`]).
#[derive(Clone, Debug)]
pub struct ScaleBoxState {
    /// The (live-scaled) box edges.
    pub segments: Vec<(DVec3, DVec3)>,
    /// Each grip with its role (`idle`, `hover`, `active`, `anchor`).
    pub grips: Vec<(DVec3, &'static str)>,
}

#[derive(Default)]
pub struct ScaleTool {
    pub lo: Option<DVec3>,
    pub hi: Option<DVec3>,
    /// The box frame's axes (`None` = the world's).
    pub axes: Option<(DVec3, DVec3, DVec3)>,
    pub orig: Option<DVec3>,
    pub grips: Vec<Grip>,
    box_version: Option<u64>,
    pub groups: Vec<GroupId>,
    pub positions: Vec<DVec3>,
    pub verts: Vec<VertexId>,
    /// The grip held.
    pub grip: Option<Grip>,
    pub hover_grip: Option<Grip>,
    pub anchor: Option<DVec3>,
    pub factors: [f64; 3],
    grabbed_px: Option<(f64, f64)>,
    moved: bool,
    pub about_center: bool,
    pub uniform: bool,
    last: Option<LastScale>,
    orig_v: Option<Vec<(MeshId, VertexId, DVec3)>>,
    orig_x: Vec<(GroupId, DMat4)>,
}

fn comp(p: DVec3, i: usize) -> f64 {
    match i {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

impl ScaleTool {
    pub fn new() -> ScaleTool {
        ScaleTool {
            factors: [1.0; 3],
            ..Default::default()
        }
    }

    fn reset(&mut self) {
        self.lo = None;
        self.hi = None;
        self.grips.clear();
        self.grip = None;
        self.hover_grip = None;
        self.anchor = None;
        self.factors = [1.0; 3];
        self.groups.clear();
        self.positions.clear();
        self.verts.clear();
        self.orig_v = None;
        self.box_version = None;
        self.grabbed_px = None;
        self.moved = false;
    }

    fn pick_frame(&mut self, vp: &Viewport) {
        let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
        let mut frame = None;
        if let [EntityRef::Group(g)] = sel.as_slice() {
            frame = vp.scene.doc.find_group(*g).and_then(group_frame);
        }
        if frame.is_none() && !axes::is_world() {
            frame = Some(axes::frame_matrix(
                axes::origin(),
                axes::axis(Axis::X),
                axes::axis(Axis::Y),
                axes::axis(Axis::Z),
            ));
        }
        let Some(f) = frame else {
            self.axes = None;
            self.orig = None;
            return;
        };
        let (o, x, y, z) = axes::frame_axes(Some(&f));
        if (x, y, z) == (DVec3::X, DVec3::Y, DVec3::Z) {
            self.axes = None;
            self.orig = None;
            return;
        }
        self.axes = Some((x, y, z));
        self.orig = Some(o);
    }

    pub fn to_local(&self, p: DVec3) -> DVec3 {
        let (Some((x, y, z)), Some(o)) = (self.axes, self.orig) else {
            return p;
        };
        let d = p - o;
        DVec3::new(d.dot(x), d.dot(y), d.dot(z))
    }

    pub fn to_world(&self, p: DVec3) -> DVec3 {
        let (Some((x, y, z)), Some(o)) = (self.axes, self.orig) else {
            return p;
        };
        o + x * p.x + y * p.y + z * p.z
    }

    pub fn selection_bounds(&mut self, vp: &Viewport) -> Option<(DVec3, DVec3)> {
        self.pick_frame(vp);
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        let mut seen = false;
        let mut absorb = |p: DVec3| {
            seen = true;
            let p = self.to_local(p);
            lo = lo.min(p);
            hi = hi.max(p);
        };
        for e in &vp.scene.selection {
            match *e {
                EntityRef::Edge(m, id) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_edge(id) {
                        let (a, b) = mesh.edge_ab(id);
                        absorb(a);
                        absorb(b);
                    }
                }
                EntityRef::Face(m, id) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_face(id) {
                        for p in mesh.face_positions(id) {
                            absorb(p);
                        }
                    }
                }
                EntityRef::Group(g) => {
                    let Some(g) = vp.scene.doc.find_group(g) else {
                        continue;
                    };
                    for (pg, m) in iter_placements(g, None) {
                        for (_, v) in vp.scene.doc.mesh(pg.mesh).verts.iter() {
                            absorb(m.map_or(v.position, |m| m.project_point(v.position)));
                        }
                    }
                }
                _ => {}
            }
        }
        seen.then_some((lo, hi))
    }

    /// (Re)build the grip box from the selection (keyed on the version).
    pub fn refresh_box(&mut self, vp: &Viewport) {
        if self.grip.is_some() || self.box_version == Some(vp.scene.version) {
            return;
        }
        self.box_version = Some(vp.scene.version);
        let b = self.selection_bounds(vp);
        self.lo = b.map(|b| b.0);
        self.hi = b.map(|b| b.1);
        self.grips.clear();
        self.hover_grip = None;
        if self.lo.is_none() {
            return;
        }
        let ext = self.extents();
        let active: Vec<usize> = (0..3).filter(|&i| ext[i] > FLAT).collect();
        if active.is_empty() {
            self.lo = None;
            self.hi = None;
            return;
        }
        let choices: Vec<Vec<f64>> = (0..3)
            .map(|i| {
                if active.contains(&i) {
                    vec![0.0, 0.5, 1.0]
                } else {
                    vec![0.5]
                }
            })
            .collect();
        for &tx in &choices[0] {
            for &ty in &choices[1] {
                for &tz in &choices[2] {
                    let params = (tx, ty, tz);
                    let t = [tx, ty, tz];
                    let mask: Vec<usize> =
                        active.iter().copied().filter(|&i| t[i] != 0.5).collect();
                    if mask.is_empty() {
                        continue;
                    }
                    self.grips.push(Grip { params, mask });
                }
            }
        }
    }

    pub fn extents(&self) -> [f64; 3] {
        let (Some(lo), Some(hi)) = (self.lo, self.hi) else {
            return [1.0; 3];
        };
        [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z]
    }

    fn active_axes(&self) -> usize {
        self.extents().iter().filter(|e| **e > FLAT).count()
    }

    pub fn grip_pos(&self, g: &Grip) -> DVec3 {
        let (lo, hi) = (self.lo.unwrap_or_default(), self.hi.unwrap_or_default());
        self.to_world(DVec3::new(
            lo.x + (hi.x - lo.x) * g.params.0,
            lo.y + (hi.y - lo.y) * g.params.1,
            lo.z + (hi.z - lo.z) * g.params.2,
        ))
    }

    /// The point opposite the grip, or the centre with About Center on.
    pub fn anchor_for(&self, g: &Grip) -> DVec3 {
        if self.about_center {
            return self
                .to_world((self.lo.unwrap_or_default() + self.hi.unwrap_or_default()) * 0.5);
        }
        let t = |i: usize| {
            if g.mask.contains(&i) {
                1.0 - g.t(i)
            } else {
                0.5
            }
        };
        self.grip_pos(&Grip {
            params: (t(0), t(1), t(2)),
            mask: g.mask.clone(),
        })
    }

    fn grip_under(&self, vp: &Viewport, sx: f64, sy: f64) -> Option<Grip> {
        let mut best = None;
        let mut best_d = vp.prefs.pick_threshold_px;
        for g in &self.grips {
            let Some(px) = vp.world_to_pixel(self.grip_pos(g)) else {
                continue;
            };
            let d = (px.x - sx).hypot(px.y - sy);
            if d < best_d {
                best = Some(g.clone());
                best_d = d;
            }
        }
        best
    }

    fn closest_on_line(o: DVec3, d: DVec3, a: DVec3, u: DVec3) -> Option<DVec3> {
        let b = d.dot(u);
        let denom = 1.0 - b * b;
        if denom.abs() < 1e-9 {
            return None;
        }
        let w = o - a;
        let (d0, e) = (d.dot(w), u.dot(w));
        Some(a + u * ((e - b * d0) / denom))
    }

    fn is_uniform(&self, g: &Grip) -> bool {
        self.uniform || g.kind(self.active_axes()) == "corner"
    }

    fn factors_from_point(&self, p: DVec3) -> Option<[f64; 3]> {
        let (g, anchor) = (self.grip.as_ref()?, self.anchor?);
        let g0 = self.grip_pos(g);
        let uniform = self.is_uniform(g);
        if uniform || g.mask.len() == 1 {
            let axis = g0 - anchor;
            let len = axis.length();
            if len < 1e-12 {
                return None;
            }
            let f = (p - anchor).dot(axis) / (len * len);
            if uniform {
                let ext = self.extents();
                return Some([0, 1, 2].map(|i| if ext[i] > FLAT { f } else { 1.0 }));
            }
            let mut fs = [1.0; 3];
            fs[g.mask[0]] = f;
            return Some(fs);
        }
        let gl = self.to_local(g0) - self.to_local(anchor);
        let pl = self.to_local(p) - self.to_local(anchor);
        let mut fs = [1.0; 3];
        for &i in &g.mask {
            let ga = comp(gl, i);
            if ga.abs() < 1e-12 {
                return None;
            }
            fs[i] = comp(pl, i) / ga;
        }
        Some(fs)
    }

    fn factors_from_cursor(&self, vp: &Viewport, sx: f64, sy: f64) -> Option<[f64; 3]> {
        let (g, anchor) = (self.grip.as_ref()?, self.anchor?);
        let g0 = self.grip_pos(g);
        let (o, d) = vp.pixel_to_ray(ScreenPt::new(sx, sy))?;
        let uniform = self.is_uniform(g);
        if uniform || g.mask.len() == 1 {
            let axis = g0 - anchor;
            let len = axis.length();
            if len < 1e-12 {
                return None;
            }
            let u = axis / len;
            let p = Self::closest_on_line(o, d, anchor, u)?;
            let f = (p - anchor).dot(u) / len;
            if uniform {
                let ext = self.extents();
                return Some([0, 1, 2].map(|i| if ext[i] > FLAT { f } else { 1.0 }));
            }
            let mut fs = [1.0; 3];
            fs[g.mask[0]] = f;
            return Some(fs);
        }
        let normal_axis = (0..3).find(|i| !g.mask.contains(i))?;
        let n = match self.axes {
            None => [DVec3::X, DVec3::Y, DVec3::Z][normal_axis],
            Some(a) => [a.0, a.1, a.2][normal_axis],
        };
        let denom = n.dot(d);
        if denom.abs() < 1e-9 {
            return None;
        }
        let t = n.dot(g0 - o) / denom;
        if t < 0.0 {
            return None;
        }
        let p = o + d * t;
        let (gl, pl) = if self.axes.is_some() {
            (
                self.to_local(g0) - self.to_local(anchor),
                self.to_local(p) - self.to_local(anchor),
            )
        } else {
            (g0 - anchor, p - anchor)
        };
        let mut fs = [1.0; 3];
        for &i in &g.mask {
            let ga = comp(gl, i);
            if ga.abs() < 1e-12 {
                return None;
            }
            fs[i] = comp(pl, i) / ga;
        }
        Some(fs)
    }

    /// The factors a hover asks for: a target snap projected on the
    /// grip's line, else the cursor.
    pub fn factors_for(&self, vp: &Viewport, ctx: &ToolContext) -> Option<[f64; 3]> {
        if let Some(s) = ctx
            .snap
            .as_ref()
            .filter(|s| TARGET_KINDS.contains(&s.kind.as_str()))
        {
            if let Some(f) = self.factors_from_point(s.point) {
                return Some(f);
            }
        }
        self.factors_from_cursor(vp, ctx.screen.0, ctx.screen.1)
    }

    /// Take a grip (`screen` is where the cursor was).
    pub fn grab(&mut self, vp: &mut Viewport, grip: Grip, screen: (f64, f64)) {
        let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
        self.groups = sel.iter().filter_map(|e| e.as_group()).collect();
        let mut positions = Vec::new();
        for e in &sel {
            match *e {
                EntityRef::Edge(m, id) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_edge(id) {
                        let (a, b) = mesh.edge_ab(id);
                        positions.extend([a, b]);
                    }
                }
                EntityRef::Face(m, id) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_face(id) {
                        positions.extend(mesh.face_positions(id));
                    }
                }
                _ => {}
            }
        }
        let mut seen: Vec<DVec3> = Vec::new();
        for p in positions {
            if !seen.iter().any(|q| (p - *q).length() < 1e-9) {
                seen.push(p);
            }
        }
        let mesh = vp.scene.mesh();
        self.verts = seen.iter().filter_map(|&p| mesh.vertex_at(p)).collect();
        self.positions = seen;
        self.anchor = Some(self.anchor_for(&grip));
        self.grip = Some(grip);
        self.factors = [1.0; 3];
        self.capture_originals(vp);
        self.grabbed_px = Some(screen);
        self.moved = false;
        vp.update();
    }

    fn capture_originals(&mut self, vp: &Viewport) {
        let mut ov = Vec::new();
        self.orig_x.clear();
        for &gid in &self.groups {
            let Some(g) = vp.scene.doc.find_group(gid) else {
                continue;
            };
            match g.xform {
                Some(x) => self.orig_x.push((gid, x)),
                None => {
                    let m = vp.scene.doc.mesh(g.mesh);
                    ov.extend(m.verts.iter().map(|(id, v)| (g.mesh, id, v.position)));
                }
            }
        }
        let mid = vp.scene.active_mesh_id();
        let mesh = vp.scene.mesh();
        ov.extend(
            self.verts
                .iter()
                .filter(|&&v| mesh.has_vertex(v))
                .map(|&v| (mid, v, mesh.pos(v))),
        );
        self.orig_v = Some(ov);
    }

    fn set_preview(&mut self, vp: &mut Viewport, factors: [f64; 3]) {
        let Some(ov) = self.orig_v.clone() else {
            return;
        };
        let identity = factors.iter().all(|f| (f - 1.0).abs() < 1e-12);
        let m = (!identity).then(|| {
            scale_matrix(
                self.anchor.unwrap_or_default(),
                ScaleFactor::PerAxis(factors),
                self.axes,
            )
        });
        for (mid, v, p0) in ov {
            let mesh = vp.scene.doc.mesh_mut(mid);
            if mesh.has_vertex(v) {
                let target = m.map_or(p0, |m| m.project_point(p0));
                let cur = mesh.pos(v);
                mesh.move_vertex(v, target - cur);
            }
        }
        for (gid, x0) in self.orig_x.clone() {
            if let Some(g) = vp.scene.doc.find_group_mut(gid) {
                g.xform = Some(m.map_or(x0, |m| m * x0));
            }
        }
        vp.scene.version += 1;
    }

    pub fn apply_preview(&mut self, vp: &mut Viewport, target: [f64; 3]) {
        if target
            .iter()
            .zip(&self.factors)
            .all(|(t, f)| (t - f).abs() < 1e-12)
        {
            return;
        }
        self.set_preview(vp, target);
        self.factors = target;
    }

    fn revert_preview(&mut self, vp: &mut Viewport) {
        if self.factors.iter().any(|f| (f - 1.0).abs() > 1e-12) {
            self.set_preview(vp, [1.0; 3]);
            self.factors = [1.0; 3];
        }
    }

    fn reanchor(&mut self, vp: &mut Viewport) {
        let Some(g) = self.grip.clone() else { return };
        let current = self.factors;
        self.apply_preview(vp, [1.0; 3]);
        self.anchor = Some(self.anchor_for(&g));
        self.apply_preview(vp, current);
        vp.update();
    }

    fn spec(&self) -> Spec {
        let grip = self.grip.as_ref();
        let uniform = self.uniform || grip.is_some_and(|g| g.kind(self.active_axes()) == "corner");
        let mask = grip.map_or(vec![0, 1, 2], |g| g.mask.clone());
        let ext = if self.lo.is_some() {
            self.extents()
        } else {
            [1.0; 3]
        };
        let active = (0..3).filter(|&i| ext[i] > FLAT).collect();
        Spec {
            uniform,
            mask,
            ext,
            active,
        }
    }

    /// A typed value onto per-axis factors for the grip held.
    pub fn typed_factors_now(&self, values: &[f64], absolute: bool) -> Option<[f64; 3]> {
        Self::typed_factors(values, absolute, &self.spec())
    }

    /// A typed value onto per-axis factors.
    fn typed_factors(values: &[f64], absolute: bool, spec: &Spec) -> Option<[f64; 3]> {
        if values.iter().any(|v| v.abs() < MIN_FACTOR) {
            return None;
        }
        let axes = if spec.uniform {
            &spec.active
        } else {
            &spec.mask
        };
        let mut fs = [1.0; 3];
        if values.len() == 1 {
            let mut f = values[0];
            if absolute {
                let base = spec.ext[*axes.first()?];
                if base <= FLAT {
                    return None;
                }
                f /= base;
            }
            for &i in axes {
                fs[i] = f;
            }
            return Some(fs);
        }
        if values.len() != axes.len() {
            return None;
        }
        for (&i, &v) in axes.iter().zip(values) {
            let mut v = v;
            if absolute {
                if spec.ext[i] <= FLAT {
                    return None;
                }
                v /= spec.ext[i];
            }
            fs[i] = v;
        }
        Some(fs)
    }

    /// Commit `factors` (one undo step).
    pub fn commit(&mut self, vp: &mut Viewport, factors: [f64; 3]) {
        self.revert_preview(vp);
        let spec = self.spec();
        let last = LastScale {
            mark: CmdMark::sentinel(),
            spec,
            anchor: self.anchor.unwrap_or_default(),
            box_axes: self.axes,
            groups: self.groups.clone(),
            positions: self.positions.clone(),
        };
        let changed = factors.iter().any(|f| (f - 1.0).abs() > 1e-9);
        if changed && factors.iter().all(|f| f.abs() >= MIN_FACTOR) {
            if let Some(mut cmd) = last.build(factors) {
                vp.execute(cmd.as_mut());
                self.last = Some(LastScale {
                    mark: CmdMark::now(vp),
                    ..last
                });
            }
        }
        self.grip = None;
        self.anchor = None;
        self.factors = [1.0; 3];
        self.orig_v = None;
        self.box_version = None;
        self.refresh_box(vp);
        vp.update();
    }

    /// Everything the overlay draws.
    pub fn scale_box_state(&self) -> Option<ScaleBoxState> {
        let (lo, hi) = (self.lo?, self.hi?);
        let m = self
            .grip
            .as_ref()
            .and(self.anchor)
            .map(|a| scale_matrix(a, ScaleFactor::PerAxis(self.factors), self.axes));
        let corner = |t: [f64; 3]| {
            let p = self.to_world(DVec3::new(
                lo.x + (hi.x - lo.x) * t[0],
                lo.y + (hi.y - lo.y) * t[1],
                lo.z + (hi.z - lo.z) * t[2],
            ));
            m.map_or(p, |m| m.project_point(p))
        };
        let ext = self.extents();
        let axes: Vec<usize> = (0..3).filter(|&i| ext[i] > FLAT).collect();
        let mut segs = Vec::new();
        if axes.len() >= 2 {
            for &axis in &axes {
                let others: Vec<usize> = (0..3).filter(|&i| i != axis).collect();
                let ta_choices: &[f64] = if ext[others[0]] <= FLAT {
                    &[0.0]
                } else {
                    &[0.0, 1.0]
                };
                let tb_choices: &[f64] = if ext[others[1]] <= FLAT {
                    &[0.0]
                } else {
                    &[0.0, 1.0]
                };
                for &ta in ta_choices {
                    for &tb in tb_choices {
                        let mut t0 = [0.0; 3];
                        t0[others[0]] = ta;
                        t0[others[1]] = tb;
                        let mut t1 = t0;
                        t1[axis] = 1.0;
                        segs.push((corner(t0), corner(t1)));
                    }
                }
            }
        } else if axes.len() == 1 {
            let mut t0 = [0.5; 3];
            let mut t1 = t0;
            t0[axes[0]] = 0.0;
            t1[axes[0]] = 1.0;
            segs.push((corner(t0), corner(t1)));
        }
        let mut grips = Vec::new();
        for g in &self.grips {
            let p = self.grip_pos(g);
            let p = m.map_or(p, |m| m.project_point(p));
            let role = if Some(g) == self.grip.as_ref() {
                "active"
            } else if Some(g) == self.hover_grip.as_ref() && self.grip.is_none() {
                "hover"
            } else {
                "idle"
            };
            grips.push((p, role));
        }
        if let (Some(_), Some(a)) = (&self.grip, self.anchor) {
            grips.push((a, "anchor"));
        }
        Some(ScaleBoxState {
            segments: segs,
            grips,
        })
    }
}

impl Tool for ScaleTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("scale"),
            shortcut: Some("S"),
            description: Some("Resize the selection by dragging the grips of its box; the corners keep its proportions."),
            ..ToolInfo::new("scale", "Scale")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: self.grip.is_some(),
            ..SnapPrefs::default()
        }
    }
    fn value_box_caption(&self) -> Option<String> {
        let Some(g) = self.grip.as_ref().or(self.hover_grip.as_ref()) else {
            return Some("Scale".into());
        };
        if self.is_uniform(g) {
            return Some("Scale".into());
        }
        let names: Vec<&str> = g.mask.iter().map(|&i| AXIS_NAMES[i]).collect();
        Some(format!("{} Scale", names.join(", ")))
    }
    /// The scale box and its grips, beneath the section planes.
    fn overlay(&self, vp: &Viewport, layer: OverlayLayer, out: &mut Vec<Paint>) {
        if layer != OverlayLayer::UnderSectionPlanes {
            return;
        }
        let Some(ScaleBoxState { segments, grips }) = self.scale_box_state() else {
            return;
        };
        let box_pen = Pen::solid(qc(214, 174, 0, 255), 1.0);
        for (a, b) in segments {
            let Some((a, b)) = vp.clip_segment_front(a, b) else {
                continue;
            };
            if let (Some(pa), Some(pb)) = (vp.world_to_pixel(a), vp.world_to_pixel(b)) {
                out.push(Paint::Line {
                    a: pa.tuple(),
                    b: pb.tuple(),
                    pen: box_pen,
                });
            }
        }
        for (pos, role) in grips {
            let Some(p) = vp.world_to_pixel(pos) else {
                continue;
            };
            let half = if matches!(role, "hover" | "active" | "anchor") {
                5.0
            } else {
                4.0
            };
            let fill = if role == "idle" {
                qc(60, 160, 60, 255)
            } else {
                qc(220, 40, 40, 255)
            };
            out.push(Paint::Rect {
                min: (p.x - half, p.y - half),
                max: (p.x + half, p.y + half),
                pen: Some(Pen::solid(qc(30, 30, 30, 255), 1.0)),
                fill: Some(fill),
            });
        }
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        self.about_center = false;
        self.uniform = false;
        self.last = None;
        self.refresh_box(vp);
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        self.reset();
        self.last = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.last = None;
        self.refresh_box(vp);
        let (sx, sy) = ctx.screen;
        if self.grip.is_some() {
            if let Some(f) = self.factors_for(vp, ctx) {
                self.commit(vp, f);
            }
            return;
        }
        if self.lo.is_some() {
            if let Some(g) = self.grip_under(vp, sx, sy) {
                self.grab(vp, g, (sx, sy));
                return;
            }
        }
        if vp.scene.selection.is_empty() {
            let (groups, positions) = gather_targets(vp, ctx);
            let mut picked: Vec<EntityRef> = groups.into_iter().map(EntityRef::Group).collect();
            if picked.is_empty() && !positions.is_empty() {
                let edge = vp.pick_edge(sx, sy).and_then(|e| e.edge);
                match edge {
                    Some((m, e)) => picked.push(EntityRef::Edge(m, e)),
                    None => {
                        if let Some((m, f)) = vp.pick_face(sx, sy) {
                            picked.push(EntityRef::Face(m, f));
                        }
                    }
                }
            }
            if !picked.is_empty() {
                vp.scene.select(picked, SelectMode::Replace);
                self.refresh_box(vp);
                vp.update();
                return;
            }
            vp.flash_status(
                &tr("Select (or click) the geometry to scale first", &[]),
                2500,
            );
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (sx, sy) = ctx.screen;
        if self.grip.is_none() {
            self.refresh_box(vp);
            let hover = if self.lo.is_some() {
                self.grip_under(vp, sx, sy)
            } else {
                None
            };
            if hover != self.hover_grip {
                self.hover_grip = hover;
                vp.update();
            }
            return;
        }
        let Some(f) = self.factors_for(vp, ctx) else {
            return;
        };
        if f.iter().any(|x| x.abs() < MIN_FACTOR) {
            return;
        }
        if let Some(g) = self.grabbed_px {
            if (sx - g.0).hypot(sy - g.1) > 4.0 {
                self.moved = true;
            }
        }
        self.apply_preview(vp, f);
        vp.update();
    }

    /// A release after a real drag commits; in place it waits for a click.
    fn on_release(&mut self, vp: &mut Viewport) {
        if self.grip.is_none() || !self.moved {
            return;
        }
        if self.factors.iter().any(|f| (f - 1.0).abs() > 1e-9) {
            let f = self.factors;
            self.commit(vp, f);
        }
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        match key {
            Key::Control => {
                self.about_center = !self.about_center;
                self.reanchor(vp);
                let msg = tr(
                    if self.about_center {
                        "About Center: on"
                    } else {
                        "About Center: off"
                    },
                    &[],
                );
                vp.flash_status(&msg, 2500);
                true
            }
            Key::Shift => {
                self.uniform = !self.uniform;
                let msg = tr(
                    if self.uniform {
                        "Scale Uniformly: on"
                    } else {
                        "Scale Uniformly: off"
                    },
                    &[],
                );
                vp.flash_status(&msg, 2500);
                vp.update();
                true
            }
            _ => false,
        }
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: true,
            absolute_length: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let (absolute, value) = match value {
            ToolValue::AbsLen(v) => (true, v.as_ref()),
            v => (false, v),
        };
        let values: Vec<f64> = match *value {
            ToolValue::Num(a) => vec![a],
            ToolValue::Pair(a, b) => vec![a, b],
            ToolValue::Triple(a, b, c) => vec![a, b, c],
            _ => return false,
        };
        if self.grip.is_some() {
            let Some(f) = Self::typed_factors(&values, absolute, &self.spec()) else {
                return false;
            };
            self.commit(vp, f);
            return true;
        }
        if let Some(last) = self.last.clone() {
            if !last.mark.is_last(vp) {
                self.last = None;
                return false;
            }
            let Some(f) = Self::typed_factors(&values, absolute, &last.spec) else {
                return false;
            };
            vp.undo();
            if let Some(mut cmd) = last.build(f) {
                vp.execute(cmd.as_mut());
            }
            self.last = Some(LastScale {
                mark: CmdMark::now(vp),
                ..last
            });
            vp.update();
            return true;
        }
        false
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        self.grip = None;
        self.anchor = None;
        self.last = None;
        vp.update();
    }

    fn snap_excluded(&self, vp: &Viewport) -> Option<SnapExcluded> {
        self.grip.as_ref()?;
        Some(SnapExcluded::of(vp, &self.verts, &self.groups).unwrap_or_default())
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let g = self.grip.as_ref()?;
        let text = if self.is_uniform(g) {
            format!("{:.2}", self.factors[g.mask[0]])
        } else {
            g.mask
                .iter()
                .map(|&i| format!("{:.2}", self.factors[i]))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let pos = if self.lo.is_some() {
            Some(self.grip_pos(g))
        } else {
            self.anchor
        };
        Some((text, pos))
    }

    fn busy(&self) -> bool {
        self.grip.is_some()
    }
}
