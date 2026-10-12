// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Push/Pull tool: extrude a face along its normal.
//!
//! Hover picks the front-most face; the first click locks onto it and
//! starts a drag along its normal; the second click (or a typed distance)
//! commits. Ctrl pushes a copy (the base stays as a dividing face); a
//! double-click repeats the last distance. The commit pipeline is
//! `xtrude_model::pushpull`; this is the gesture around it.
//!
//! While a push goes into the solid, the frame leaves out what it replaces
//! — the base face, and the walls it cuts down with their edges — and the
//! preview draws what stays of those walls.

use crate::input::ScreenPt;
use std::cell::Cell;

use glam::DVec3;
use xtrude_core::math::VecExt;
use xtrude_core::mesh::Key;
use xtrude_core::topology::tkey;
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::pushpull::{
    swept_by_push, PushOutcome, PushPull, SweepRing, WallCut, MIN_EXTRUDE,
};
use xtrude_model::units::fmt_len;

use super::{Plane, PreviewFace, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::tool_any;
use crate::viewport::Viewport;

thread_local! {
    /// The last committed distance, shared across activations: a
    /// double-click repeats it.
    static LAST_DISTANCE: Cell<Option<f64>> = const { Cell::new(None) };
}

pub fn last_distance() -> Option<f64> {
    LAST_DISTANCE.with(|c| c.get())
}

pub fn set_last_distance(d: Option<f64>) {
    LAST_DISTANCE.with(|c| c.set(d));
}

/// What the distance inference locked onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceKind {
    Vertex,
    GuidePoint,
    GuideLine,
    Edge,
    Face,
}

#[derive(Default)]
pub struct PushPullTool {
    pub hovered_face: Option<(MeshId, FaceId)>,
    hover_group: Option<GroupId>,
    /// The push in progress (the drag state of the doc engine).
    pub pp: PushPull,
    pub inference_point: Option<DVec3>,
    pub inference_kind: Option<InferenceKind>,
    light_faces: Vec<PreviewFace>,
    light_rings: Vec<SweepRing>,
    /// The walls a push into the solid cuts down: hidden while previewing,
    /// what stays of them drawn among `light_faces`.
    light_cut: WallCut,
    /// Per-drag candidate vertices for the distance inference.
    infer_cache: Option<Vec<DVec3>>,
}

impl PushPullTool {
    pub fn new() -> PushPullTool {
        PushPullTool::default()
    }

    pub fn dragging(&self) -> bool {
        self.pp.dragging()
    }

    pub fn extrusion(&self) -> f64 {
        self.pp.extrusion
    }

    pub fn set_extrusion(&mut self, e: f64) {
        self.pp.extrusion = e;
    }

    fn reset(&mut self) {
        self.hovered_face = None;
        self.hover_group = None;
        self.pp.reset();
        self.inference_point = None;
        self.inference_kind = None;
        self.infer_cache = None;
        self.light_faces.clear();
        self.light_rings.clear();
        self.light_cut = WallCut::default();
    }

    /// A face inside a group that is not open is not pushable — and the
    /// refusal has to speak.
    fn refuse_closed_group(vp: &mut Viewport) {
        vp.flash_status(
            &tr(
                "That face belongs to a group — double-click to open it first, then push inside",
                &[],
            ),
            4000,
        );
    }

    /// Lock onto `face` (the drag-start click).
    pub fn begin(&mut self, vp: &mut Viewport, mesh: MeshId, face: FaceId) {
        self.pp.begin(&mut vp.scene, mesh, face);
        self.infer_cache = None;
    }

    pub fn clamp(&mut self, vp: &mut Viewport) {
        if let Some(l) = self.pp.clamp_extrusion() {
            let msg = tr(
                "Push limited to {value} — deeper would leave the solid",
                &[("value", &fmt_len(l))],
            );
            vp.flash_status(&msg, 5000);
        }
    }

    pub fn revert_preview(&mut self, vp: &mut Viewport) {
        self.pp.revert_prism_preview(&mut vp.scene);
    }

    /// Does the sweep move the cap away from the camera?
    fn sweeps_away_from_eye(&self, vp: &Viewport) -> bool {
        let (Some(anchor), Some(n)) = (self.pp.anchor, self.pp.normal) else {
            return self.pp.extrusion < 0.0;
        };
        let Some(camera) = vp.probe().camera(vp) else {
            return self.pp.extrusion < 0.0;
        };
        let off = n * self.pp.extrusion;
        off.dot(anchor - camera.eye()) > 0.0
    }

    pub fn show_light_preview(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        let (mut faces, rings, cut) = match self.pp.target {
            Some(t) => {
                let mesh = vp.scene.doc.mesh(t);
                let (faces, rings) = self.pp.light_faces(mesh);
                (faces, rings, self.pp.wall_cut(mesh))
            }
            None => (Vec::new(), Vec::new(), WallCut::default()),
        };
        // A cut side's sweep is the strip that comes off its wall: what
        // stays of the wall is drawn instead. `faces` is the cap, then one
        // quad per side, ring after ring.
        if !faces.is_empty() && !cut.sides.is_empty() {
            let mut first = vec![1];
            for (low, _) in &rings {
                first.push(first[first.len() - 1] + low.len());
            }
            let quads: Vec<usize> = cut.sides.iter().map(|&(r, i)| first[r] + i).collect();
            faces = faces
                .into_iter()
                .enumerate()
                .filter(|(k, _)| !quads.contains(k))
                .map(|(_, f)| f)
                .chain(cut.remains.iter().cloned())
                .collect();
        }
        self.light_faces = faces;
        self.light_rings = rings;
        let recessing = !self.light_faces.is_empty()
            && self.pp.base_face.is_some()
            && self.pp.attached
            && !self.pp.keep_base
            && (self.sweeps_away_from_eye(vp) || !cut.walls.is_empty());
        match (recessing, self.pp.target, self.pp.base_face) {
            (true, Some(t), Some(f)) => {
                let mesh = vp.scene.doc.mesh(t);
                let mut edges = Vec::new();
                for &w in &cut.walls {
                    for lp in mesh.face(w).loops() {
                        let n = lp.len();
                        edges.extend((0..n).filter_map(|i| mesh.find_edge(lp[i], lp[(i + 1) % n])));
                    }
                }
                let faces = std::iter::once(f).chain(cut.walls.iter().copied());
                vp.set_suppressed(faces.map(|f| (t, f)), edges.into_iter().map(|e| (t, e)));
            }
            _ => vp.set_suppressed([], []),
        }
        self.light_cut = cut;
        vp.update();
    }

    pub fn show_prism_preview(&mut self, vp: &mut Viewport) {
        self.light_faces.clear();
        self.light_rings.clear();
        self.light_cut = WallCut::default();
        vp.set_suppressed([], []);
        self.pp.show_prism_preview(&mut vp.scene);
        vp.update();
    }

    /// Commit the push (one undo step) and say what happened.
    pub fn commit(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
        self.light_faces.clear();
        self.light_rings.clear();
        self.light_cut = WallCut::default();
        vp.set_suppressed([], []);
        self.revert_preview(vp);
        if self.pp.base_face.is_none() || self.pp.extrusion.abs() < MIN_EXTRUDE {
            self.reset();
            vp.update();
            return;
        }
        match self.pp.commit(&mut vp.scene, &mut vp.history) {
            PushOutcome::ToppedOut => {
                let msg = tr(
                    "Push stopped at {d}: going further would break the solid",
                    &[("d", &fmt_len(self.pp.extrusion.abs()))],
                );
                vp.flash_status(&msg, 4000);
                set_last_distance(Some(self.pp.extrusion));
            }
            PushOutcome::Refused => {
                vp.flash_status(
                    &tr(
                        "Push refused: would break the solid (left it unchanged)",
                        &[],
                    ),
                    6000,
                );
            }
            PushOutcome::Committed => set_last_distance(Some(self.pp.extrusion)),
            PushOutcome::NoOp => {}
        }
        self.reset();
        vp.update();
    }

    /// The vertices the inference may lock onto: the active mesh and every
    /// group's mesh (each once), in their stored coordinates.
    fn candidate_vertices(&mut self, vp: &Viewport) -> &[DVec3] {
        if self.infer_cache.is_none() {
            let mut seen: Vec<MeshId> = Vec::new();
            let mut out = Vec::new();
            let meshes = std::iter::once(vp.scene.active_mesh_id())
                .chain(vp.scene.doc.groups.iter().map(|g| g.mesh));
            for m in meshes {
                if seen.contains(&m) {
                    continue;
                }
                seen.push(m);
                out.extend(vp.scene.doc.mesh(m).verts.iter().map(|(_, v)| v.position));
            }
            self.infer_cache = Some(out);
        }
        self.infer_cache.as_deref().unwrap_or_default()
    }

    /// The distance making the moved face level with the geometry under
    /// the cursor: a corner, a guide point, a guide line, the hovered edge,
    /// the face under the cursor. Corners and edges hidden behind the model
    /// and the walls the push slides along give none.
    pub fn infer_reference_distance(
        &mut self,
        vp: &mut Viewport,
        ctx: &ToolContext,
    ) -> Option<f64> {
        let (anchor, normal) = (self.pp.anchor?, self.pp.normal?);
        let (sx, sy) = ctx.screen;
        let thr = vp.prefs.snap_threshold_px;
        let exclude: std::collections::HashSet<Key> =
            self.pp.cap_positions.iter().map(|&p| tkey(p)).collect();
        let mut best: Option<(f64, f64, DVec3, InferenceKind)> = None;
        let cands: Vec<DVec3> = self.candidate_vertices(vp).to_vec();
        for p in cands {
            let Some(pix) = vp.world_to_pixel(p) else {
                continue;
            };
            let dd = (pix.x - sx).powi(2) + (pix.y - sy).powi(2);
            if dd > thr * thr || exclude.contains(&tkey(p)) {
                continue;
            }
            if best.is_none_or(|b| dd < b.0) && !vp.is_occluded(p) {
                best = Some((dd, (p - anchor).dot(normal), p, InferenceKind::Vertex));
            }
        }
        let guides = vp.scene.doc.guides.clone();
        for g in guides.iter().filter(|g| g.direction.is_none()) {
            let Some(pix) = vp.world_to_pixel(g.point) else {
                continue;
            };
            let dd = (pix.x - sx).powi(2) + (pix.y - sy).powi(2);
            if dd <= thr * thr && best.is_none_or(|b| dd < b.0) {
                best = Some((
                    dd,
                    (g.point - anchor).dot(normal),
                    g.point,
                    InferenceKind::GuidePoint,
                ));
            }
        }
        if let Some((_, dist, p, kind)) = best {
            self.inference_point = Some(p);
            self.inference_kind = Some(kind);
            return Some(dist);
        }
        if let Some((on, dist)) =
            self.infer_guide_line(vp, &guides, ScreenPt::new(sx, sy), thr, anchor, normal)
        {
            self.inference_point = Some(on);
            self.inference_kind = Some(InferenceKind::GuideLine);
            return Some(dist);
        }
        if let Some(edge) = vp.inference.hover_edge.clone() {
            let (a, b) = (edge.a, edge.b);
            let ab = b - a;
            if (!exclude.contains(&tkey(a)) || !exclude.contains(&tkey(b))) && ab.length() > 1e-9 {
                let on = vp.project_to_lock_line(a, ab, sx, sy);
                let t = (on - a).dot(ab) / ab.dot(ab);
                let on = a + ab * t.clamp(0.0, 1.0);
                let dist = (on - anchor).dot(normal);
                if dist.abs() >= MIN_EXTRUDE && !vp.is_occluded(on) {
                    self.inference_point = Some(on);
                    self.inference_kind = Some(InferenceKind::Edge);
                    return Some(dist);
                }
            }
        }
        if let Some(hit) = vp.pick_face_placement(sx, sy) {
            let own = hit.placement.is_none() && Some(hit.mesh) == self.pp.target;
            let is_base = own && Some(hit.face) == self.pp.base_face;
            if !is_base && !(own && self.is_own_wall(vp, hit.face)) {
                if let Some((o, d)) = vp.pixel_to_ray(ScreenPt::new(sx, sy)) {
                    let (fpt, fnrm) = vp.face_plane_world(&hit);
                    let fnrm = fnrm.unit_or_zero();
                    let denom = fnrm.dot(d);
                    if denom.abs() >= 1e-6 {
                        let t = fnrm.dot(fpt - o) / denom;
                        if t > 0.0 {
                            let p = o + d * t;
                            let dist = (p - anchor).dot(normal);
                            if dist.abs() >= MIN_EXTRUDE {
                                self.inference_point = Some(p);
                                self.inference_kind = Some(InferenceKind::Face);
                                return Some(dist);
                            }
                        }
                    }
                }
            }
        }
        self.inference_point = None;
        self.inference_kind = None;
        None
    }

    /// Whether `face` of the target is a wall the push slides along: it
    /// carries one of the base's edges and holds the push axis, so the
    /// point under the cursor on it is no level to stop at.
    fn is_own_wall(&self, vp: &Viewport, face: FaceId) -> bool {
        let (Some(t), Some(base), Some(n)) = (self.pp.target, self.pp.base_face, self.pp.normal)
        else {
            return false;
        };
        let mesh = vp.scene.doc.mesh(t);
        if !mesh.has_face(face) || !mesh.has_face(base) {
            return false;
        }
        let carries_base_edge = mesh.face(base).loops().any(|lp| {
            let k = lp.len();
            (0..k).any(|i| {
                mesh.find_edge(lp[i], lp[(i + 1) % k])
                    .is_some_and(|e| mesh.edge(e).faces.contains(&face))
            })
        });
        carries_base_edge && swept_by_push(mesh, face, n)
    }

    fn infer_guide_line(
        &self,
        vp: &Viewport,
        guides: &[xtrude_model::guide::Guide],
        at: ScreenPt,
        thr: f64,
        anchor: DVec3,
        normal: DVec3,
    ) -> Option<(DVec3, f64)> {
        let (sx, sy) = (at.x, at.y);
        let lines: Vec<&xtrude_model::guide::Guide> =
            guides.iter().filter(|g| g.direction.is_some()).collect();
        if lines.is_empty() {
            return None;
        }
        let (origin, dir) = vp.pixel_to_ray(ScreenPt::new(sx, sy))?;
        let mut best: Option<(f64, DVec3, f64)> = None;
        for g in lines {
            let Some(gd) = g.direction else { continue };
            if gd.dot(normal).abs() > 0.999 {
                continue;
            }
            let w0 = g.point - origin;
            let (a, b, c) = (gd.dot(gd), gd.dot(dir), dir.dot(dir));
            let (d, e) = (gd.dot(w0), dir.dot(w0));
            let den = a * c - b * b;
            if den.abs() < 1e-12 {
                continue;
            }
            let s = (b * e - c * d) / den;
            let on = g.point + gd * s;
            let Some(pix) = vp.world_to_pixel(on) else {
                continue;
            };
            let dd = (pix.x - sx).powi(2) + (pix.y - sy).powi(2);
            if dd > thr * thr {
                continue;
            }
            let dist = (on - anchor).dot(normal);
            if dist.abs() < MIN_EXTRUDE {
                continue;
            }
            if best.is_none_or(|bb| dd < bb.0) {
                best = Some((dd, on, dist));
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// Lock the face under the cursor for a drag, or refuse a group's.
    fn start_on(&mut self, vp: &mut Viewport, x: f64, y: f64, use_hover: bool) -> bool {
        let (face, grp) = if use_hover {
            match self.hovered_face {
                None => return false,
                Some(f) => (f, self.hover_group),
            }
        } else {
            match vp.pick_face_any(x, y) {
                None => return false,
                Some(h) => ((h.mesh, h.face), h.owner),
            }
        };
        if grp.is_some() {
            Self::refuse_closed_group(vp);
            return false;
        }
        self.begin(vp, face.0, face.1);
        true
    }
}

impl Tool for PushPullTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("pushpull"),
            shortcut: Some("P"),
            shortcut_alt: Some("U"),
            description: Some("Push or pull a face to extrude it into a solid or cut it back."),
            wireframe_depth_tested: true,
            ..ToolInfo::new("pushpull", "Push / Pull")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            hover_group_edges: true,
            ..SnapPrefs::default()
        }
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Distance".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.revert_preview(vp);
        vp.set_hover(None);
        vp.set_suppressed([], []);
        self.reset();
    }

    /// Mid-drag, the plane through the anchor that contains the push axis
    /// and faces the camera.
    fn drag_plane(&self, vp: &Viewport) -> Option<Plane> {
        if !self.pp.dragging() {
            return None;
        }
        let (anchor, n) = (self.pp.anchor?, self.pp.normal?);
        let forward = vp.camera.target - vp.camera.eye();
        let side = forward - n * forward.dot(n);
        if side.length() < 1e-6 {
            return None;
        }
        Some((anchor, side.unit_or_zero()))
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if !self.pp.dragging() {
            self.inference_point = None;
            self.inference_kind = None;
            match vp.pick_face_any(ctx.screen.0, ctx.screen.1) {
                Some(h) => {
                    self.hovered_face = Some((h.mesh, h.face));
                    self.hover_group = h.owner;
                    let shade = if h.owner.is_some() {
                        None
                    } else {
                        Some(EntityRef::Face(h.mesh, h.face))
                    };
                    vp.set_hover(shade);
                }
                None => {
                    self.hovered_face = None;
                    self.hover_group = None;
                    vp.set_hover(None);
                }
            }
            return;
        }
        let (Some(anchor), Some(normal)) = (self.pp.anchor, self.pp.normal) else {
            return;
        };
        self.pp.keep_base = ctx.modifiers.ctrl;
        let cap_moved = self.pp.prism_applied != 0.0;
        self.revert_preview(vp);
        if cap_moved {
            // The hover's edge pick saw the cap where the last move left
            // it: its edges would hand back the previous distance.
            vp.inference.hover_edge = vp.pick_edge_any(ctx.screen.0, ctx.screen.1);
        }
        match self.infer_reference_distance(vp, ctx) {
            Some(d) => self.pp.extrusion = d,
            None => {
                let p = vp.project_to_lock_line(anchor, normal, ctx.screen.0, ctx.screen.1);
                self.pp.extrusion = (p - anchor).dot(normal);
            }
        }
        self.clamp(vp);
        if self.pp.prism_cap && !self.pp.keep_base {
            self.show_prism_preview(vp);
        } else {
            self.show_light_preview(vp);
        }
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.pp.keep_base = ctx.modifiers.ctrl;
        if !self.pp.dragging() {
            let keep = self.pp.keep_base;
            if self.start_on(vp, ctx.screen.0, ctx.screen.1, true) {
                self.pp.keep_base = keep;
                vp.set_hover(None);
                vp.update();
            }
            return;
        }
        if self.pp.extrusion.abs() < MIN_EXTRUDE {
            return;
        }
        self.commit(vp);
    }

    /// Repeat the last committed distance on the face under the cursor.
    fn on_double_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(last) = last_distance() else {
            self.on_click(vp, ctx);
            return;
        };
        let keep = ctx.modifiers.ctrl;
        self.pp.keep_base = keep;
        if !self.pp.dragging() {
            if !self.start_on(vp, ctx.screen.0, ctx.screen.1, false) {
                return;
            }
            self.pp.keep_base = keep;
        } else if self.pp.extrusion.abs() > MIN_EXTRUDE {
            self.commit(vp);
            return;
        }
        self.pp.extrusion = last;
        self.clamp(vp);
        self.commit(vp);
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(value) = value.as_num() else {
            return false;
        };
        if !self.pp.dragging() || value == 0.0 {
            return false;
        }
        let sign = if self.pp.extrusion < 0.0 { -1.0 } else { 1.0 };
        self.pp.extrusion = sign * value;
        self.clamp(vp);
        self.commit(vp);
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.light_faces.clear();
        self.light_rings.clear();
        self.light_cut = WallCut::default();
        self.revert_preview(vp);
        vp.set_hover(None);
        vp.set_suppressed([], []);
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let mut segs = PushPull::rubber_band_lines_cut(&self.light_rings, &self.light_cut.sides);
        // The cut walls' edges are hidden: their remains draw the outline.
        for rest in &self.light_cut.remains {
            for lp in std::iter::once(&rest.vertices).chain(&rest.holes) {
                let n = lp.len();
                segs.extend((0..n).map(|i| (lp[i], lp[(i + 1) % n])));
            }
        }
        segs
    }

    fn preview_faces(&self) -> Vec<PreviewFace> {
        self.light_faces.clone()
    }

    fn inference_marker(&self) -> Option<(DVec3, InferenceKind)> {
        if !self.pp.dragging() {
            return None;
        }
        Some((self.inference_point?, self.inference_kind?))
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        if !self.pp.dragging() {
            return None;
        }
        let (anchor, n) = (self.pp.anchor?, self.pp.normal?);
        let mid = anchor + n * (self.pp.extrusion * 0.5);
        Some((fmt_len(self.pp.extrusion.abs()), Some(mid)))
    }

    fn busy(&self) -> bool {
        self.pp.dragging()
    }

    fn busy_for_delete(&self) -> bool {
        self.pp.base_face.is_some()
    }
}
