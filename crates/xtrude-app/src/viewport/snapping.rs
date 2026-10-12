// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The snap engine's view of the scene, the centre reference, encouraged
//! points and the tool context of a pointer event.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use glam::{DMat4, DVec3};
use xtrude_core::axes;
use xtrude_core::math::{MatExt, VecExt};
use xtrude_core::snap::{
    self, compute_snap, LinearMode, SnapEdge, SnapKind, SnapQuery, SnapResult,
};
use xtrude_core::Axis;
use xtrude_core::{EdgeId, FaceId};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{
    frame_from_points, group_frame, oriented_bounds, oriented_box_corners, placement_points,
};
use xtrude_model::ids::{GroupId, MeshId};

use super::pick::active_cut;
use super::{PickedEdge, Viewport};
use crate::input::Modifiers;
use crate::tools::ToolContext;

/// How close (px) the cursor must come to an acquired circle centre for
/// its dot to show (capped share of the circle's size on screen).
pub const CENTER_HINT_PX: f64 = 40.0;
/// A circle smaller than this on screen offers no centre.
pub const CENTER_MIN_RADIUS_PX: f64 = 12.0;
/// How long the cursor must rest on a point to encourage it.
pub const ENCOURAGE_MS: u64 = 250;
/// How close (px) a component origin / arc midpoint must be to snap.
pub const POINT_INFERENCE_PX: f64 = 40.0;
/// Above this many loose edges, only those near the cursor are snapped.
pub const LOOSE_SNAP_CAP: usize = 3000;

/// Snap kinds whose point can be encouraged.
pub const ENCOURAGING_KINDS: [&str; 8] = [
    "endpoint",
    "midpoint",
    "arc_midpoint",
    "center",
    "component_origin",
    "origin",
    "intersection",
    "close",
];

/// Inferences whose direction a Shift lock can capture.
const SHIFT_LOCKABLE: [&str; 6] = [
    "axis",
    "axis_inference",
    "reference",
    "through_point",
    "perp_face",
    "extension",
];

/// What the active tool has in motion, as the picks see it: the loose
/// edges (active mesh) and the placements of the groups in motion and of
/// the groups nested in them.
#[derive(Clone, Debug, Default)]
pub struct InMotion {
    pub edges: HashSet<EdgeId>,
    pub placements: Vec<GroupId>,
}

impl InMotion {
    /// Whether the picked edge (loose, or a group's by its owner) moves.
    pub fn has_edge(&self, e: &PickedEdge) -> bool {
        e.edge.is_some_and(|(_, id)| self.edges.contains(&id))
            || e.group.is_some_and(|g| self.placements.contains(&g))
    }
}

/// Where an acquired centre came from.
#[derive(Clone, Debug, PartialEq)]
pub enum CenterSource {
    Edge(EdgeId),
    Face(FaceId),
}

/// Which arc of its source a centre is.
#[derive(Clone, Debug, PartialEq)]
pub enum CenterKey {
    Edge(i64),
    Face(FaceId, (usize, usize)),
}

/// The centre of the last circle or arc the cursor visited.
#[derive(Clone, Debug, PartialEq)]
pub struct CenterRef {
    pub center: DVec3,
    pub radius: f64,
    pub key: CenterKey,
    pub version: u64,
    pub source: CenterSource,
    pub mesh: MeshId,
    /// The placement the curve lives in (its matrix maps the centre).
    pub group: Option<GroupId>,
    pub xform: Option<DMat4>,
}

impl Viewport {
    /// The scene the snap engine sees: loose edges, guides, the group edges
    /// near the cursor, component origins, arc midpoints, selected group
    /// box corners and the acquired centre.
    pub fn snap_scene(&mut self, px: Option<(f64, f64)>) -> Vec<SnapEdge> {
        let mut lines: Vec<SnapEdge> = Vec::new();
        let guides = self.scene.doc.guides.clone();
        for g in &guides {
            if g.is_line() {
                if let Some((a, b)) = self.guide_snap_segment(g, 1.5) {
                    lines.push(SnapEdge {
                        a,
                        b,
                        guide: true,
                        ..Default::default()
                    });
                }
            } else {
                lines.push(SnapEdge::segment(g.point, g.point));
            }
        }
        let excl = self.active_tool().and_then(|t| t.snap_excluded(self));
        let mut near: Vec<SnapEdge> = match px {
            Some((x, y)) => self.nearby_group_edges(x, y, 48.0, 48),
            None => Vec::new(),
        };
        if let Some(ex) = &excl {
            if !ex.groups.is_empty() {
                near.retain(|e| e.group.is_none_or(|g| !ex.groups.contains(&g)));
            }
        }
        let skip: Vec<GroupId> = excl
            .as_ref()
            .map(|e| e.groups.iter().copied().collect())
            .unwrap_or_default();
        if let Some((x, y)) = px {
            let moving = self.moving_placements(&skip);
            near.extend(self.component_origin_points(x, y, &moving));
            near.extend(self.arc_midpoints(x, y, &moving));
        }
        near.extend(self.selection_box_points(&skip));
        if let Some(r) = self.valid_center_ref() {
            if let Some(pc) = self.world_to_pixel(r.center) {
                if self.center_hint_px(&r, pc.tuple()) > 0.0 {
                    near.push(SnapEdge {
                        a: r.center,
                        b: r.center,
                        center: true,
                        ..Default::default()
                    });
                }
            }
        }
        let cut = active_cut(&self.scene).cloned();
        let kept = |e: &SnapEdge| match &cut {
            None => true,
            Some(sp) => !(sp.side(e.a) > 1e-6 && sp.side(e.b) > 1e-6),
        };
        near.retain(|e| kept(e));
        let n_loose = self.scene.mesh().edges.len();
        let big = px.is_some() && n_loose > LOOSE_SNAP_CAP;
        let active = self.scene.active_mesh_id();
        let loose_ids: Vec<EdgeId> = if big {
            let (x, y) = px.unwrap_or_default();
            self.nearby_loose_edges(x, y, 64.0, 400)
        } else {
            self.scene.mesh().edge_ids()
        };
        let simple = lines.is_empty() && near.is_empty() && cut.is_none() && !big && excl.is_none();
        let mesh = self.scene.doc.mesh(active);
        let mut out: Vec<SnapEdge> = Vec::with_capacity(loose_ids.len() + lines.len() + near.len());
        for e in loose_ids {
            let ed = mesh.edge(e);
            if !simple && ed.hidden {
                continue;
            }
            if let Some(ex) = &excl {
                if ex.edges.contains(&e) {
                    continue;
                }
            }
            let se = SnapEdge {
                a: mesh.pos(ed.v0),
                b: mesh.pos(ed.v1),
                soft: ed.soft,
                hidden: ed.hidden,
                curve: ed.curve,
                ..Default::default()
            };
            if !simple && !kept(&se) {
                continue;
            }
            out.push(se);
        }
        out.extend(lines);
        out.extend(near);
        out
    }

    /// What the active tool has in motion; `None` when nothing moves (or a
    /// copy leaves the original in place).
    pub fn in_motion(&self) -> Option<InMotion> {
        let ex = self.active_tool().and_then(|t| t.snap_excluded(self))?;
        let groups: Vec<GroupId> = ex.groups.iter().copied().collect();
        Some(InMotion {
            edges: ex.edges,
            placements: self.moving_placements(&groups),
        })
    }

    /// The groups in motion and every group nested in them: a nested
    /// placement moves with its parent.
    pub fn moving_placements(&self, groups: &[GroupId]) -> Vec<GroupId> {
        let mut found = Vec::new();
        for &id in groups {
            if let Some(g) = self.scene.doc.find_group(id) {
                g.walk(&mut found);
            }
        }
        found.into_iter().map(|g| g.id).collect()
    }

    /// Each placed group's own origin near the cursor, as a degenerate
    /// pseudo-edge ("Component origin"); none from the groups in `skip`.
    pub fn component_origin_points(&mut self, px: f64, py: f64, skip: &[GroupId]) -> Vec<SnapEdge> {
        let mut out = Vec::new();
        for p in self.placements() {
            if skip.contains(&p.group) {
                continue;
            }
            let Some(xf) = p.xform else { continue };
            let o = xf.project_point(DVec3::ZERO);
            let Some(s) = self.world_to_pixel(o) else {
                continue;
            };
            if (s.x - px).hypot(s.y - py) <= POINT_INFERENCE_PX {
                out.push(SnapEdge {
                    a: o,
                    b: o,
                    component_origin: true,
                    ..Default::default()
                });
            }
        }
        out
    }

    /// The sweep midpoint of every open curve chain (loose and in every
    /// placement but those of the groups in `skip`) near the cursor ("Arc
    /// midpoint").
    pub fn arc_midpoints(&mut self, px: f64, py: f64, skip: &[GroupId]) -> Vec<SnapEdge> {
        let mut mids: Vec<DVec3> = mesh_arc_midpoints(self.scene.mesh());
        for p in self.placements() {
            if skip.contains(&p.group) {
                continue;
            }
            let lm = mesh_arc_midpoints(self.scene.doc.mesh(p.mesh));
            mids.extend(
                lm.into_iter()
                    .map(|m| p.xform.map_or(m, |x| x.project_point(m))),
            );
        }
        mids.into_iter()
            .filter(|m| {
                self.world_to_pixel(*m)
                    .is_some_and(|pc| (pc.x - px).hypot(pc.y - py) <= POINT_INFERENCE_PX)
            })
            .map(|m| SnapEdge {
                a: m,
                b: m,
                arc_midpoint: true,
                ..Default::default()
            })
            .collect()
    }

    /// `(frame, lo, hi)` of a group in its own axes.
    pub fn group_obb(&self, group: GroupId) -> Option<((DVec3, DVec3, DVec3), DVec3, DVec3)> {
        let g = self.scene.doc.find_group(group)?;
        let pos = placement_points(&self.scene.doc.meshes, g);
        if let Some(fr) = group_frame(g) {
            let (_o, x, y, z) = axes::frame_axes(Some(&fr));
            return Some(oriented_bounds(&pos, Some((x, y, z))));
        }
        let world = oriented_bounds(&pos, None);
        let own = oriented_bounds(&pos, Some(frame_from_points(&pos)));
        let volume = |b: &((DVec3, DVec3, DVec3), DVec3, DVec3)| {
            let (_f, lo, hi) = b;
            let mut side = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
            side.sort_by(f64::total_cmp);
            side[1] * side[2] * side[0].max(1e-6)
        };
        Some(if volume(&own) < 0.9 * volume(&world) {
            own
        } else {
            world
        })
    }

    /// The corners of each selected group's box, as endpoints.
    pub fn selection_box_points(&self, skip: &[GroupId]) -> Vec<SnapEdge> {
        let mut out = Vec::new();
        for e in &self.scene.selection {
            let EntityRef::Group(g) = *e else { continue };
            if skip.contains(&g) {
                continue;
            }
            if let Some((f, lo, hi)) = self.group_obb(g) {
                for p in oriented_box_corners(f, lo, hi) {
                    out.push(SnapEdge::segment(p, p));
                }
            }
        }
        out
    }

    // ---- centre reference ----------------------------------------------------------------
    /// Acquire the centre of the circle or arc under the cursor.
    pub fn update_center_ref(&mut self, x: f64, y: f64) -> Option<CenterRef> {
        let mut found = None;
        if let Some(edge) = self.inference.hover_edge.clone() {
            if let (Some((m, e)), Some(_)) = (edge.edge, edge.curve) {
                found = self.center_of_edge(m, e, None, None);
            } else if edge.in_group && edge.group.is_some() {
                found = self.center_of_group_edge(&edge);
            }
        }
        if found.is_none() {
            if let Some(hit) = self.pick_face_placement(x, y) {
                found = self.center_of_face(hit.mesh, hit.face, hit.placement, hit.xform, x, y);
            }
        }
        if let Some(f) = found {
            self.inference.center_ref = Some(f.clone());
            return Some(f);
        }
        self.valid_center_ref();
        None
    }

    fn center_of_group_edge(&self, pseudo: &super::PickedEdge) -> Option<CenterRef> {
        let g = self.scene.doc.find_group(pseudo.group?)?;
        let mesh = self.scene.doc.mesh(g.mesh);
        if mesh.edges.is_empty() {
            return None;
        }
        let xf = g.xform;
        let (la, lb) = match xf {
            Some(m) => {
                let inv = m.try_inverse()?;
                (inv.project_point(pseudo.a), inv.project_point(pseudo.b))
            }
            None => (pseudo.a, pseudo.b),
        };
        let k = |p: DVec3| {
            (
                xtrude_core::math::round_even(p.x, 4).to_bits(),
                xtrude_core::math::round_even(p.y, 4).to_bits(),
                xtrude_core::math::round_even(p.z, 4).to_bits(),
            )
        };
        let want = [k(la), k(lb)];
        let edge = mesh.edge_ids().into_iter().find(|&e| {
            let (a, b) = mesh.edge_ab(e);
            let got = [k(a), k(b)];
            (got[0] == want[0] && got[1] == want[1]) || (got[0] == want[1] && got[1] == want[0])
        })?;
        mesh.edge(edge).curve?;
        let mut found = self.center_of_edge(g.mesh, edge, Some(g.id), None)?;
        if let Some(m) = xf {
            found.center = m.project_point(found.center);
            found.xform = Some(m);
        }
        Some(found)
    }

    fn center_of_edge(
        &self,
        mesh_id: MeshId,
        edge: EdgeId,
        group: Option<GroupId>,
        xform: Option<DMat4>,
    ) -> Option<CenterRef> {
        let mesh = self.scene.doc.mesh(mesh_id);
        let curve = mesh.edge(edge).curve?;
        let mut pts: Vec<(xtrude_core::VertexId, DVec3)> = Vec::new();
        for e in mesh.curve_edges(edge) {
            let ed = mesh.edge(e);
            for v in [ed.v0, ed.v1] {
                if !pts.iter().any(|(id, _)| *id == v) {
                    pts.push((v, mesh.pos(v)));
                }
            }
        }
        if pts.len() < 3 {
            return None;
        }
        let positions: Vec<DVec3> = pts.iter().map(|p| p.1).collect();
        let (c, r) = snap::fit_circle(&positions, 0.01)?;
        Some(CenterRef {
            center: c,
            radius: r,
            key: CenterKey::Edge(curve),
            version: self.scene.version,
            source: CenterSource::Edge(edge),
            mesh: mesh_id,
            group,
            xform,
        })
    }

    fn center_of_face(
        &self,
        mesh_id: MeshId,
        face: FaceId,
        group: Option<GroupId>,
        xform: Option<DMat4>,
        x: f64,
        y: f64,
    ) -> Option<CenterRef> {
        let mesh = self.scene.doc.mesh(mesh_id);
        let centers = snap::curve_centers_of_face(mesh, face, xform.as_ref());
        let (c, r, key) = match centers.len() {
            0 => return None,
            1 => centers[0],
            _ => {
                let mut best: Option<(f64, DVec3, f64, (usize, usize))> = None;
                for (c, r, key) in centers {
                    let Some(pc) = self.world_to_pixel(c) else {
                        continue;
                    };
                    let d = (pc.x - x).hypot(pc.y - y);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, c, r, key));
                    }
                }
                let (_, c, r, key) = best?;
                (c, r, key)
            }
        };
        Some(CenterRef {
            center: c,
            radius: r,
            key: CenterKey::Face(face, key),
            version: self.scene.version,
            source: CenterSource::Face(face),
            mesh: mesh_id,
            group,
            xform,
        })
    }

    /// How close the cursor must come for the centre's dot to show; zero
    /// when the circle is too small on screen.
    pub fn center_hint_px(&self, r: &CenterRef, pc: (f64, f64)) -> f64 {
        let mut r_px: f64 = 0.0;
        for q in [
            self.world_to_pixel(r.center + DVec3::new(r.radius, 0.0, 0.0)),
            self.world_to_pixel(r.center + DVec3::new(0.0, r.radius, 0.0)),
        ]
        .into_iter()
        .flatten()
        {
            r_px = r_px.max((q.x - pc.0).hypot(q.y - pc.1));
        }
        if r_px < CENTER_MIN_RADIUS_PX {
            return 0.0;
        }
        CENTER_HINT_PX.min(0.5 * r_px)
    }

    /// The acquired centre, re-checked against the scene: gone with its
    /// circle, following it when it moved.
    pub fn valid_center_ref(&mut self) -> Option<CenterRef> {
        let r = self.inference.center_ref.clone()?;
        if r.version == self.scene.version {
            return Some(r);
        }
        let fresh = if !self.scene.doc.meshes.contains_key(&r.mesh) {
            None
        } else {
            let mesh = self.scene.doc.mesh(r.mesh);
            match (&r.source, &r.key) {
                (CenterSource::Face(f), CenterKey::Face(_, k)) => {
                    if mesh.has_face(*f) {
                        snap::curve_centers_of_face(mesh, *f, r.xform.as_ref())
                            .into_iter()
                            .find(|c| c.2 == *k)
                            .map(|(c, rad, _)| CenterRef {
                                center: c,
                                radius: rad,
                                version: self.scene.version,
                                ..r.clone()
                            })
                    } else {
                        None
                    }
                }
                (CenterSource::Edge(e), _) => {
                    if mesh.has_edge(*e) {
                        self.center_of_edge(r.mesh, *e, r.group, r.xform)
                            .map(|mut f| {
                                if let Some(m) = r.xform {
                                    f.center = m.project_point(f.center);
                                }
                                f
                            })
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        self.inference.center_ref = fresh.clone();
        fresh
    }

    pub fn center_ref(&self) -> Option<&CenterRef> {
        self.inference.center_ref.as_ref()
    }

    // ---- encouraged points ------------------------------------------------------------------
    /// The cursor is over `point` (or nothing): (re)start the pause that
    /// turns it into an encouraged point.
    pub fn dwell_on(&mut self, point: Option<DVec3>) {
        self.inference.dwell_on(point);
    }

    /// The dwell timer fired: encourage the point rested on.
    pub fn encourage_dwelt(&mut self) {
        if let Some(p) = self.inference.dwell_point {
            self.encourage_point(p);
        }
        self.inference.dwell_since = None;
    }

    /// Fire the timers whose time has come by `now`: the dwell, the
    /// note beside the pointer. `MainWindow::tick` calls this at the start
    /// of every frame, before the frame's input; the shell asks for a frame
    /// when `next_tick_in` says, so the timers fire in a window left alone.
    pub fn tick(&mut self, now: Instant) {
        if self.dwell_due_in(now).is_some_and(|d| d.is_zero()) {
            self.encourage_dwelt();
        }
        self.expire_note(now);
    }

    /// How long from `now` until `tick` has something to fire, `None`
    /// when nothing waits.
    pub fn next_tick_in(&self, now: Instant) -> Option<Duration> {
        match (self.dwell_due_in(now), self.note_due_in(now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// How long from `now` until the dwell timer fires (zero when
    /// overdue), `None` while no timer runs.
    pub fn dwell_due_in(&self, now: Instant) -> Option<Duration> {
        let due = self.inference.dwell_since? + Duration::from_millis(ENCOURAGE_MS);
        Some(due.saturating_duration_since(now))
    }

    pub fn dwell_point(&self) -> Option<DVec3> {
        self.inference.dwell_point
    }

    /// The dwell timer is running.
    pub fn dwell_active(&self) -> bool {
        self.inference.dwell_since.is_some()
    }

    /// Make `point` the newest encouraged point (two are kept) and the
    /// 'from point' reference.
    pub fn encourage_point(&mut self, point: DVec3) {
        self.inference.encourage(point);
        self.update();
    }

    // ---- the tool context ---------------------------------------------------------------------
    /// The Measure tool's cue: before its first click, the cursor ON a model axis.
    fn axis_source_cue(&self, snap: SnapResult, px: f64, py: f64) -> SnapResult {
        let Some(tool) = self.active_tool() else {
            return snap;
        };
        if !tool.snap_prefs().axis_source || tool.start_point().is_some() {
            return snap;
        }
        if !(snap.kind == SnapKind::None || snap.kind == SnapKind::OnFace) {
            return snap;
        }
        let Some(name) = self.pick_axis(px, py) else {
            return snap;
        };
        let axis = axes::axis(name);
        let o = axes::origin();
        let foot = o + axis * (snap.point - o).dot(axis);
        let mut r = SnapResult::new(foot, SnapKind::OnAxis, name.color());
        r.axis = Some(name);
        r
    }

    fn run_snap(&mut self, world_raw: DVec3, px: f64, py: f64, shift: bool) -> SnapResult {
        let tool = self.active_tool();
        let chain_first = tool.and_then(|t| t.chain_first_point());
        let start_pt = tool.and_then(|t| t.start_point());
        let prefs = tool.map(|t| t.snap_prefs());
        let magnetic = prefs.and_then(|p| p.magnetic_axis_deg);
        let screen_axis = prefs.and_then(|p| p.screen_axis_px);
        let radial = prefs.is_some_and(|p| p.radial_arm);
        let face_under = self.pick_face_any(px, py).is_some();
        let edges = self.snap_scene(Some((px, py)));
        // The occlusion test needs the viewport mutably (its pick cache)
        // while the projection reads it: both go through one cell.
        self.pick_index();
        let reference_edge = self.inference.reference_edge.as_ref().map(|e| (e.a, e.b));
        let (reference_mode, axis_lock, threshold, angle, edge_thr) = (
            self.inference.reference_mode,
            self.inference.axis_lock,
            self.prefs.snap_threshold_px,
            self.prefs.inference_angle_deg,
            self.prefs.edge_snap_threshold_px,
        );
        let (acq_edge, acq_point, acq_face, acq_points, shift_lock, linear, wp_normal) = (
            self.inference.acquired_edge,
            self.inference.acquired_point,
            self.inference.acquired_face_normal,
            self.inference.encouraged.clone(),
            self.inference.shift_lock,
            self.inference.linear_inference_mode,
            self.work_plane_normal(),
        );
        let snap = {
            let me = std::cell::RefCell::new(&mut *self);
            let w2p = |p: DVec3| me.borrow().world_to_pixel(p).map(|p| p.tuple());
            let lock = |s: DVec3, d: DVec3| me.borrow().project_to_lock_line(s, d, px, py);
            let occl = |p: DVec3| me.borrow_mut().is_occluded(p);
            let mut q = SnapQuery::new(world_raw, (px, py), &w2p, threshold);
            q.project_onto_line = Some(&lock);
            q.chain_first_point = chain_first;
            q.start_point = start_pt;
            q.axis_lock = axis_lock;
            q.shift_held = shift;
            q.reference_edge = reference_edge;
            q.reference_mode = reference_mode;
            q.inference_angle_deg = angle;
            q.is_occluded = Some(&occl);
            q.face_under_cursor = face_under;
            q.edge_threshold_px = Some(edge_thr);
            q.magnetic_axis_deg = magnetic;
            q.screen_axis_px = screen_axis;
            q.acquired_edge = acq_edge;
            q.acquired_point = acq_point;
            q.acquired_face_normal = acq_face;
            q.acquired_points = acq_points;
            q.shift_lock_dir = shift_lock.map(|s| s.0);
            q.shift_lock_color = shift_lock.map(|s| s.1);
            q.linear_mode = linear;
            q.work_plane_normal = wp_normal;
            q.radial_arm = radial;
            compute_snap(&q, &edges)
        };
        self.axis_source_cue(snap, px, py)
    }

    /// The tool context for a pointer event at `(x, y)` — the snapped point
    /// on the current work plane — or `None` when the cursor ray misses it.
    pub fn build_ctx(&mut self, x: f64, y: f64, modifiers: Modifiers) -> Option<ToolContext> {
        self.sync_axes();
        // `ev.position().toPoint()`: whole pixels.
        let (px, py) = (x.round(), y.round());
        let world_raw = self.world_from_pixel(px, py)?;
        let uses_snap = self.active_tool().is_none_or(|t| t.snap_prefs().uses_snap);
        if !uses_snap {
            return Some(ToolContext {
                world: world_raw,
                screen: (x, y),
                modifiers,
                snap: Some(SnapResult::new(world_raw, SnapKind::None, snap::COLOR_NONE)),
            });
        }
        let snap = self.run_snap(world_raw, px, py, modifiers.shift);
        Some(ToolContext {
            world: snap.point,
            screen: (x, y),
            modifiers,
            snap: Some(snap),
        })
    }

    /// Re-run the snap at the last cursor position (a lock or modifier
    /// changed without the mouse moving).
    pub fn refresh_snap(&mut self) {
        self.refresh_snap_with(self.input.held_modifiers);
    }

    pub(crate) fn refresh_snap_with(&mut self, modifiers: Modifiers) {
        self.sync_axes();
        self.update();
        let Some(pos) = self.input.last_mouse_pos else {
            return;
        };
        if !self.active_tool().is_some_and(|t| t.snap_prefs().uses_snap) {
            return;
        }
        let (px, py) = (pos.x.round(), pos.y.round());
        let Some(world_raw) = self.world_from_pixel(px, py) else {
            return;
        };
        let snap = self.run_snap(world_raw, px, py, modifiers.shift);
        self.inference.last_snap = Some(snap.clone());
        let ctx = ToolContext {
            world: snap.point,
            screen: pos.tuple(),
            modifiers,
            snap: Some(snap),
        };
        self.with_tool(|t, vp| t.on_hover(vp, &ctx));
        self.emit_measurement();
    }

    /// On Shift press, freeze the active inference's direction.
    pub fn capture_shift_lock(&mut self) {
        self.inference.shift_lock = None;
        let Some(snap) = self.inference.last_snap.clone() else {
            return;
        };
        let Some(start) = self.tool_start_point() else {
            return;
        };
        if !SHIFT_LOCKABLE.contains(&snap.kind.as_str()) {
            return;
        }
        let d = match snap.axis {
            Some(a) => {
                let mut d = axis_vector(a);
                if (snap.point - start).dot(d) < 0.0 {
                    d = -d;
                }
                d
            }
            None => snap.point - start,
        };
        if d.length() > 1e-6 {
            self.inference.shift_lock = Some((d.unit_or_zero(), snap.color));
        }
    }

    pub fn linear_mode(&self) -> LinearMode {
        self.inference.linear_inference_mode
    }
}

/// The world axis vectors the Shift lock reads.
fn axis_vector(a: Axis) -> DVec3 {
    match a {
        Axis::X => DVec3::X,
        Axis::Y => DVec3::Y,
        _ => DVec3::Z,
    }
}

/// The sweep midpoint of every open curve chain of `mesh` (mesh space).
pub fn mesh_arc_midpoints(mesh: &xtrude_core::Mesh) -> Vec<DVec3> {
    use std::collections::BTreeMap;
    let mut by_curve: BTreeMap<i64, Vec<EdgeId>> = BTreeMap::new();
    let mut order: Vec<i64> = Vec::new();
    for e in mesh.edge_ids() {
        if let Some(c) = mesh.edge(e).curve {
            if !by_curve.contains_key(&c) {
                order.push(c);
            }
            by_curve.entry(c).or_default().push(e);
        }
    }
    let mut mids = Vec::new();
    for c in order {
        let edges = &by_curve[&c];
        let mut degree: Vec<(xtrude_core::VertexId, usize, DVec3)> = Vec::new();
        for &e in edges {
            let ed = mesh.edge(e);
            for v in [ed.v0, ed.v1] {
                match degree.iter_mut().find(|d| d.0 == v) {
                    Some(d) => d.1 += 1,
                    None => degree.push((v, 1, mesh.pos(v))),
                }
            }
        }
        let ends = degree.iter().filter(|d| d.1 == 1).count();
        if ends != 2 || degree.len() < 3 {
            continue;
        }
        let pts: Vec<DVec3> = degree.iter().map(|d| d.2).collect();
        let Some((c, r)) = snap::fit_circle(&pts, 0.01) else {
            continue;
        };
        let m = pts.iter().fold(DVec3::ZERO, |s, p| s + (*p - c));
        if m.length() < 1e-9 {
            continue;
        }
        mids.push(c + m.unit_or_zero() * r);
    }
    mids
}
