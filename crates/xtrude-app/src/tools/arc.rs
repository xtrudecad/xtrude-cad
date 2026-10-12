// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Arc tools.
//!
//! The 2-point arc: the chord's two ends, then a bulge. A start ON an edge
//! previews an arc tangent to it (cyan); at the same distance from the
//! shared corner on the adjacent edge it turns magenta — the fillet, which
//! a double-click draws and trims. Also the 3-point arc, the centre arc and
//! the sector.

use std::cell::Cell;
use std::f64::consts::PI;

use glam::DVec3;
use xtrude_core::axes::plane_axes;
use xtrude_core::math::{MatExt, VecExt};
use xtrude_core::snap::{self, COLOR_MIDPOINT, COLOR_TANGENT};
use xtrude_core::topology::point_on_seg_incl;
use xtrude_core::{EdgeId, Mesh, VertexId};
use xtrude_model::commands::{
    DeleteEdgesCommand, RebuildPlanarFacesCommand, RebuildPlaneFacesCommand, TagCurveCommand,
};
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::Command;
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_len;

use super::circle::{busy_plane, flat_drawing};
use super::{
    world_per_pixel, Plane, PlaneLock, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue,
    WorkPlane,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

/// Polyline segments approximating an arc.
pub const SEGMENTS: usize = 16;
/// Magenta of the arc tangent to BOTH edges of a corner.
pub const COLOR_FILLET: [f64; 3] = [0.85, 0.30, 0.80];
/// Within this many pixels of the tangent bulge, the arc snaps to it.
pub const TANGENT_PX: f64 = 8.0;
/// Within this many pixels of the equidistant point, the end snaps to it.
pub const FILLET_PX: f64 = 10.0;
/// A double-click this close to a two-edge corner repeats the last fillet.
pub const CORNER_PX: f64 = 20.0;

thread_local! {
    /// Distance corner → tangent points of the last fillet (shared by every
    /// arc tool).
    static LAST_FILLET_D: Cell<Option<f64>> = const { Cell::new(None) };
}

pub fn last_fillet_d() -> Option<f64> {
    LAST_FILLET_D.with(|c| c.get())
}

pub fn set_last_fillet_d(d: Option<f64>) {
    LAST_FILLET_D.with(|c| c.set(d));
}

fn rgba(c: [f64; 3]) -> Rgba {
    [c[0], c[1], c[2], 1.0]
}

fn circumcenter2(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<(f64, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (
        a.0 * a.0 + a.1 * a.1,
        b.0 * b.0 + b.1 * b.1,
        c.0 * c.0 + c.1 * c.1,
    );
    let ux = (a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d;
    let uy = (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d;
    Some((ux, uy))
}

fn wrap(mut a: f64) -> f64 {
    while a <= -PI {
        a += 2.0 * PI;
    }
    while a > PI {
        a -= 2.0 * PI;
    }
    a
}

/// 2D points of the circular arc from `s` to `e` through `m`, in
/// `segments` spans (the straight chord when collinear).
pub fn arc_3pts_2d(
    s: (f64, f64),
    m: (f64, f64),
    e: (f64, f64),
    segments: usize,
) -> Vec<(f64, f64)> {
    let Some((cx, cy)) = circumcenter2(s, m, e) else {
        return vec![s, e];
    };
    let r = (s.0 - cx).hypot(s.1 - cy);
    let a0 = (s.1 - cy).atan2(s.0 - cx);
    let a1 = (e.1 - cy).atan2(e.0 - cx);
    let am = (m.1 - cy).atan2(m.0 - cx);
    let mut d = wrap(a1 - a0);
    let da = wrap(am - a0);
    if d >= 0.0 && !(0.0 <= da && da <= d) {
        d -= 2.0 * PI;
    } else if d < 0.0 && !(d <= da && da <= 0.0) {
        d += 2.0 * PI;
    }
    (0..=segments)
        .map(|k| {
            let t = a0 + d * (k as f64 / segments as f64);
            (cx + r * t.cos(), cy + r * t.sin())
        })
        .collect()
}

/// Commit an arc polyline through the shared curve pipeline. `close_to`
/// (a centre) closes the wedge (Sector); `trim` erases the corner stubs a
/// fillet leaves.
pub fn commit_arc(
    vp: &mut Viewport,
    pts: &[DVec3],
    close_to: Option<DVec3>,
    trim: Option<Vec<(DVec3, DVec3)>>,
) {
    let mut segs: Vec<(DVec3, DVec3)> = pts.windows(2).map(|w| (w[0], w[1])).collect();
    if let Some(c) = close_to {
        segs.insert(0, (c, pts[0]));
        segs.push((pts[pts.len() - 1], c));
    }
    let tag = || -> Box<dyn Command> {
        Box::new(TagCurveCommand {
            pts: pts.to_vec(),
            closed: false,
        })
    };
    let mut after: Vec<Box<dyn Command>> = Vec::new();
    if let Some(t) = trim {
        after.push(Box::new(DeleteEdgesCommand {
            endpoints: t,
            cascade_faces: true,
        }));
    }
    let cmd = if flat_drawing(&vp.scene, pts) {
        let mut extra: Vec<Box<dyn Command>> =
            vec![tag(), Box::new(RebuildPlanarFacesCommand::new())];
        extra.extend(after);
        build_add_edges(&vp.scene, &segs, false, extra, None)
    } else if let Some((o, n)) = busy_plane(&vp.scene, pts) {
        let mut extra: Vec<Box<dyn Command>> =
            vec![tag(), Box::new(RebuildPlaneFacesCommand::new(o, n))];
        extra.extend(after);
        build_add_edges(&vp.scene, &segs, true, extra, None)
    } else {
        let mut extra: Vec<Box<dyn Command>> = vec![tag()];
        extra.extend(after);
        build_add_edges(&vp.scene, &segs, true, extra, None)
    };
    vp.execute_boxed(cmd);
}

/// What the bulge snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulgeKind {
    Tangent,
    Fillet,
    Half,
}

/// The 2-point arc.
pub struct ArcTool {
    pub start_point: Option<DVec3>,
    pub end_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
    /// Polyline segments of the next arc ("Ns").
    pub segments: usize,
    pub tangent_dir: Option<DVec3>,
    pub snap_bulge: Option<f64>,
    pub bulge_kind: Option<BulgeKind>,
    /// The edge the start sits on (interior): `(edge, P1)`.
    pub start_edge: Option<(EdgeId, DVec3)>,
    /// The equidistant point the cursor snapped to: `(V, P2, edge B)`.
    pub equidistant: Option<(DVec3, DVec3, EdgeId)>,
    /// `(V, P1, P2)` of the corner this arc rounds.
    pub fillet: Option<(DVec3, DVec3, DVec3)>,
    pub fillet_edge_b: Option<EdgeId>,
    /// The last arc committed (undo depth, scene version) and its apex
    /// parameters, so "Ns" right after rebuilds it.
    last_cmd: Option<(usize, u64)>,
    last_params: Option<(ApexParams, Option<Trim>)>,
    last_apex: Cell<Option<ApexParams>>,
    wireframe: Option<Rgba>,
}

/// The segments a fillet arc trims off its corner.
type Trim = Vec<(DVec3, DVec3)>;

/// `(apex 2D, end 2D, (u, v, origin))` of the last arc laid out.
#[derive(Clone, Copy, Debug)]
pub struct ApexParams {
    apex: (f64, f64),
    end: (f64, f64),
    u: DVec3,
    v: DVec3,
    origin: DVec3,
}

impl Default for ArcTool {
    fn default() -> Self {
        ArcTool::new()
    }
}

impl ArcTool {
    pub fn new() -> ArcTool {
        ArcTool {
            start_point: None,
            end_point: None,
            hover_point: None,
            work_plane: None,
            lock: PlaneLock::default(),
            segments: SEGMENTS,
            tangent_dir: None,
            snap_bulge: None,
            bulge_kind: None,
            start_edge: None,
            equidistant: None,
            fillet: None,
            fillet_edge_b: None,
            last_cmd: None,
            last_params: None,
            last_apex: Cell::new(None),
            wireframe: None,
        }
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.end_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
        self.lock.clear_plane_lock();
        self.tangent_dir = None;
        self.snap_bulge = None;
        self.bulge_kind = None;
        self.start_edge = None;
        self.equidistant = None;
        self.fillet = None;
        self.fillet_edge_b = None;
        self.last_apex.set(None);
        self.wireframe = None;
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
            Some(s) => std::iter::once(s)
                .chain(self.end_point.or(self.hover_point))
                .collect(),
        }
    }

    pub fn axes(&self) -> (DVec3, DVec3) {
        plane_axes(
            self.lock
                .drawing_plane(self.work_plane, &self.plane_points())
                .1,
        )
    }

    fn to2(&self, p: DVec3, u: DVec3, v: DVec3) -> (f64, f64) {
        let d = p - self.start_point.unwrap_or(p);
        (d.dot(u), d.dot(v))
    }

    /// Signed perpendicular distance from the chord midpoint to `cursor`.
    pub fn bulge_for(&self, cursor: DVec3) -> f64 {
        let Some(end) = self.end_point else {
            return 0.0;
        };
        let (u, v) = self.axes();
        let e2 = self.to2(end, u, v);
        let len = e2.0.hypot(e2.1);
        if len < 1e-9 {
            return 0.0;
        }
        let (px, py) = (-e2.1 / len, e2.0 / len);
        let mid = (e2.0 / 2.0, e2.1 / 2.0);
        let c2 = self.to2(cursor, u, v);
        (c2.0 - mid.0) * px + (c2.1 - mid.1) * py
    }

    /// The arc's points: the bulge given, else the snapped one, else the
    /// cursor's; `end` defaults to the end point.
    pub fn points(
        &self,
        cursor: Option<DVec3>,
        bulge: Option<f64>,
        end: Option<DVec3>,
    ) -> Vec<DVec3> {
        let (Some(start), Some(end)) = (self.start_point, end.or(self.end_point)) else {
            return Vec::new();
        };
        let (u, v) = self.axes();
        let e2 = self.to2(end, u, v);
        let len = e2.0.hypot(e2.1);
        if len < 1e-9 {
            return Vec::new();
        }
        let h = match (bulge, self.snap_bulge) {
            (Some(b), _) => b,
            (None, Some(b)) => b,
            (None, None) => self.bulge_for(cursor.unwrap_or(end)),
        };
        if h.abs() < 1e-4 {
            return vec![start, end];
        }
        let (px, py) = (-e2.1 / len, e2.0 / len);
        let mid = (e2.0 / 2.0, e2.1 / 2.0);
        let apex = (mid.0 + px * h, mid.1 + py * h);
        self.last_apex.set(Some(ApexParams {
            apex,
            end: e2,
            u,
            v,
            origin: start,
        }));
        arc_3pts_2d((0.0, 0.0), apex, e2, self.segments)
            .into_iter()
            .map(|(x, y)| start + u * x + v * y)
            .collect()
    }

    /// `(edge, P)` when `P` lies inside an edge of the mesh drawn on — the
    /// hovered edge first, then a search.
    fn edge_under(vp: &Viewport, p: DVec3) -> Option<(EdgeId, DVec3)> {
        let mesh = vp.scene.mesh();
        let interior = |e: EdgeId| {
            let (a, b) = mesh.edge_ab(e);
            point_on_seg_incl(p, a, b, 1e-4)
                && (p - a).length() > 1e-4
                && (p - b).length() > 1e-4
                && (b - a).length() > 1e-6
        };
        if let Some(h) = &vp.inference.hover_edge {
            if let Some((m, e)) = h.edge {
                if m == vp.scene.active_mesh_id() && mesh.has_edge(e) && interior(e) {
                    return Some((e, p));
                }
            }
        }
        mesh.edge_ids()
            .into_iter()
            .find(|&e| interior(e))
            .map(|e| (e, p))
    }

    /// The direction the arc leaves the start along its edge, toward `toward`.
    fn edge_tangent(&self, vp: &Viewport, toward: DVec3) -> Option<DVec3> {
        let (edge, p1) = self.start_edge?;
        let mesh = vp.scene.mesh();
        if !mesh.has_edge(edge) {
            return None;
        }
        let (a, b) = mesh.edge_ab(edge);
        let d = b - a;
        if d.length() < 1e-9 {
            return None;
        }
        let mut d = d.unit_or_zero();
        if d.dot(toward - p1) < 0.0 {
            d = -d;
        }
        Some(d)
    }

    fn pixel_distance(vp: &Viewport, screen: (f64, f64), p: DVec3) -> Option<f64> {
        let q = vp.world_to_pixel(p)?;
        Some((q.x - screen.0).hypot(q.y - screen.1))
    }

    fn equidistant_near(
        &self,
        vp: &Viewport,
        screen: (f64, f64),
    ) -> Option<(DVec3, DVec3, EdgeId)> {
        let (edge, p1) = self.start_edge?;
        let mesh = vp.scene.mesh();
        if !mesh.has_edge(edge) {
            return None;
        }
        let ed = mesh.edge(edge);
        let mut best: Option<(f64, DVec3, DVec3, EdgeId)> = None;
        for vid in [ed.v0, ed.v1] {
            let vpos = mesh.pos(vid);
            let d = (p1 - vpos).length();
            if d < 1e-6 {
                continue;
            }
            for &b in &mesh.verts[vid].edges {
                if b == edge {
                    continue;
                }
                let w = mesh.pos(mesh.edge(b).other(vid));
                let span = (w - vpos).length();
                if span < 1e-9 || d > span + 1e-6 {
                    continue;
                }
                let p2 = vpos + (w - vpos) * (d / span);
                let Some(dist) = Self::pixel_distance(vp, screen, p2) else {
                    continue;
                };
                if dist > FILLET_PX {
                    continue;
                }
                if best.is_none_or(|bb| dist < bb.0) {
                    best = Some((dist, vpos, p2, b));
                }
            }
        }
        best.map(|b| (b.1, b.2, b.3))
    }

    /// `(V, A, B)`: the two-edge corner near the cursor whose edges both
    /// hold the last fillet.
    fn corner_near(vp: &Viewport, screen: (f64, f64)) -> Option<(VertexId, EdgeId, EdgeId)> {
        let d = last_fillet_d().filter(|d| *d != 0.0)?;
        let mesh = vp.scene.mesh();
        let mut best: Option<(f64, VertexId, EdgeId, EdgeId)> = None;
        for vid in mesh.vertex_ids() {
            let edges = &mesh.verts[vid].edges;
            if edges.len() != 2 {
                continue;
            }
            let (a, b) = (edges[0], edges[1]);
            if mesh.edge(a).faces.is_empty() && mesh.edge(b).faces.is_empty() {
                continue;
            }
            if mesh.edge_length(a) < d - 1e-6 || mesh.edge_length(b) < d - 1e-6 {
                continue;
            }
            let Some(dist) = Self::pixel_distance(vp, screen, mesh.pos(vid)) else {
                continue;
            };
            if dist > CORNER_PX {
                continue;
            }
            if best.is_none_or(|bb| dist < bb.0) {
                best = Some((dist, vid, a, b));
            }
        }
        best.map(|b| (b.1, b.2, b.3))
    }

    fn round_corner(
        &mut self,
        vp: &mut Viewport,
        corner: (VertexId, EdgeId, EdgeId),
        d: f64,
        m: Modifiers,
    ) {
        let (vid, a, b) = corner;
        let mesh = vp.scene.mesh();
        let vpos = mesh.pos(vid);
        let pa = mesh.pos(mesh.edge(a).other(vid));
        let pb = mesh.pos(mesh.edge(b).other(vid));
        let p1 = vpos + (pa - vpos).unit_or_zero() * d;
        let p2 = vpos + (pb - vpos).unit_or_zero() * d;
        self.start_point = Some(p1);
        self.end_point = Some(p2);
        self.start_edge = Some((a, p1));
        self.fillet = Some((vpos, p1, p2));
        let n = (pa - vpos).cross(pb - vpos);
        self.lock.hover_plane = (n.length() >= 1e-9).then(|| (vpos, n.unit_or_zero()));
        self.tangent_dir = Some((vpos - p1).unit_or_zero());
        let Some(h) = self.tangent_bulge(None, None) else {
            self.reset();
            return;
        };
        self.snap_bulge = Some(h);
        self.bulge_kind = Some(BulgeKind::Fillet);
        let pts = self.points(None, Some(h), None);
        if pts.len() >= 2 {
            let trim = self.corner_trim(vp, m);
            self.commit(vp, &pts, trim);
        } else {
            self.reset();
        }
    }

    /// The two corner stubs to erase after a fillet: only for an arc that
    /// IS the fillet, on a face, at a two-edge corner, without Alt.
    fn corner_trim(&self, vp: &Viewport, m: Modifiers) -> Option<Vec<(DVec3, DVec3)>> {
        let (v, p1, p2) = self.fillet?;
        if self.bulge_kind != Some(BulgeKind::Fillet) || m.alt {
            return None;
        }
        let (edge, _) = self.start_edge?;
        let mesh = vp.scene.mesh();
        if !mesh.has_edge(edge) || mesh.edge(edge).faces.is_empty() {
            return None;
        }
        let vert = mesh.vertex_at(v)?;
        if mesh.verts[vert].edges.len() != 2 {
            return None;
        }
        Some(vec![(v, p1), (v, p2)])
    }

    /// The direction an existing arc leaves its END vertex when that vertex
    /// is our start point.
    fn tangent_at_start(&self, vp: &Viewport) -> Option<DVec3> {
        let start = self.start_point?;
        if let Some(t) = tangent_in_mesh(vp.scene.mesh(), start) {
            return Some(t);
        }
        for p in vp.placements() {
            let gm = vp.scene.doc.mesh(p.mesh);
            if gm.edges.is_empty() {
                continue;
            }
            let pt = match p.xform {
                Some(x) => match x.try_inverse() {
                    Some(inv) => inv.project_point(start),
                    None => continue,
                },
                None => start,
            };
            let Some(mut t) = tangent_in_mesh(gm, pt) else {
                continue;
            };
            if let Some(x) = p.xform {
                t = x.transform_vector(t);
                if t.length() < 1e-9 {
                    continue;
                }
                t = t.unit_or_zero();
            }
            return Some(t);
        }
        None
    }

    /// The signed bulge that makes the arc leave the start along `tangent`
    /// toward `end`: sagitta (L/2)·tan(a/2).
    pub fn tangent_bulge(&self, end: Option<DVec3>, tangent: Option<DVec3>) -> Option<f64> {
        let tangent = tangent.or(self.tangent_dir)?;
        let end = end.or(self.end_point)?;
        let (u, v) = self.axes();
        let e2 = self.to2(end, u, v);
        let len = e2.0.hypot(e2.1);
        if len < 1e-9 {
            return None;
        }
        let (cx, cy) = (e2.0 / len, e2.1 / len);
        let (tx, ty) = (tangent.dot(u), tangent.dot(v));
        let tl = tx.hypot(ty);
        if tl < 1e-6 {
            return None;
        }
        let (tx, ty) = (tx / tl, ty / tl);
        let angle = (cx * ty - cy * tx).atan2(cx * tx + cy * ty);
        if (angle.abs() - PI).abs() < 1e-3 {
            return None;
        }
        Some((len / 2.0) * (angle / 2.0).tan())
    }

    fn snap_bulge_for(&mut self, vp: &Viewport, ctx: &ToolContext) {
        let (Some(start), Some(end)) = (self.start_point, self.end_point) else {
            return;
        };
        let (u, v) = self.axes();
        let e2 = self.to2(end, u, v);
        let len = e2.0.hypot(e2.1);
        if len < 1e-9 {
            return;
        }
        let perp = u * (-e2.1 / len) + v * (e2.0 / len);
        let mid = (start + end) * 0.5;
        let Some(scale) = world_per_pixel(vp, mid, perp) else {
            return;
        };
        let tol = scale * TANGENT_PX;
        let b = self.bulge_for(ctx.world);
        if self.tangent_dir.is_some() {
            if let Some(ht) = self.tangent_bulge(None, None) {
                if (b - ht).abs() <= tol {
                    self.snap_bulge = Some(ht);
                    let fil = self.fillet.is_some();
                    self.bulge_kind = Some(if fil {
                        BulgeKind::Fillet
                    } else {
                        BulgeKind::Tangent
                    });
                    self.wireframe = Some(rgba(if fil { COLOR_FILLET } else { COLOR_TANGENT }));
                    return;
                }
            }
        }
        let half = len / 2.0;
        if (b.abs() - half).abs() <= tol {
            self.snap_bulge = Some(half.copysign(if b != 0.0 { b } else { 1.0 }));
            self.bulge_kind = Some(BulgeKind::Half);
            self.wireframe = Some(rgba(COLOR_MIDPOINT));
        }
    }

    fn commit(&mut self, vp: &mut Viewport, pts: &[DVec3], trim: Option<Vec<(DVec3, DVec3)>>) {
        let apex = self.last_apex.get();
        if trim.is_some() {
            if let Some((v, p1, _)) = self.fillet {
                set_last_fillet_d(Some((p1 - v).length()));
            }
        }
        commit_arc(vp, pts, None, trim.clone());
        self.last_cmd = Some((vp.history.undo_len(), vp.scene.version));
        self.last_params = None;
        if let Some(a) = apex {
            if pts.len() >= 3 {
                self.last_params = Some((a, trim));
            }
        }
        self.reset();
        vp.update();
    }

    /// The armed fillet with `radius`: tangent points at r / tan(θ/2) from
    /// the corner; a radius the edges cannot hold is refused, and said.
    fn fillet_with_radius(&mut self, vp: &mut Viewport, radius: f64) -> bool {
        let Some((v, p1, p2)) = self.fillet else {
            return false;
        };
        let (a, b) = (p1 - v, p2 - v);
        if radius <= 0.0 || a.length() < 1e-9 || b.length() < 1e-9 {
            return false;
        }
        let (ua, ub) = (a.unit_or_zero(), b.unit_or_zero());
        let theta = ua.dot(ub).clamp(-1.0, 1.0).acos();
        if theta < 1e-3 || (theta - PI).abs() < 1e-3 {
            return false;
        }
        let d = radius / (theta / 2.0).tan();
        let mesh = vp.scene.mesh();
        let reach: Vec<f64> = [self.start_edge.map(|s| s.0), self.fillet_edge_b]
            .into_iter()
            .flatten()
            .filter(|e| mesh.has_edge(*e))
            .map(|e| mesh.edge_length(e))
            .collect();
        if let Some(min) = reach.iter().cloned().reduce(f64::min) {
            if d > min + 1e-6 {
                let msg = tr(
                    "Radius {r} needs {d} of edge on each side of the corner; the shorter one is {e}",
                    &[("r", &fmt_len(radius)), ("d", &fmt_len(d)), ("e", &fmt_len(min))],
                );
                vp.flash_status(&msg, 2500);
                return true;
            }
        }
        let s = v + ua * d;
        let e = v + ub * d;
        self.start_point = Some(s);
        self.end_point = Some(e);
        if let Some((edge, _)) = self.start_edge {
            self.start_edge = Some((edge, s));
        }
        self.fillet = Some((v, s, e));
        self.tangent_dir = Some((v - s).unit_or_zero());
        let Some(h) = self.tangent_bulge(None, None) else {
            return false;
        };
        self.snap_bulge = Some(h);
        self.bulge_kind = Some(BulgeKind::Fillet);
        let pts = self.points(None, Some(h), None);
        if pts.len() < 2 {
            return false;
        }
        let trim = self.corner_trim(vp, Modifiers::NONE);
        self.commit(vp, &pts, trim);
        true
    }

    /// "2r": the radius instead of the bulge (the minor arc).
    pub fn on_radius_value(&mut self, vp: &mut Viewport, radius: f64) -> bool {
        let (Some(start), Some(end)) = (self.start_point, self.end_point) else {
            return false;
        };
        if radius <= 0.0 {
            return false;
        }
        let half = (end - start).length() * 0.5;
        if half < 1e-9 {
            return false;
        }
        if radius < half - 1e-9 {
            let msg = tr(
                "Radius {r} is smaller than half the chord ({h})",
                &[("r", &fmt_len(radius)), ("h", &fmt_len(half))],
            );
            vp.flash_status(&msg, 2500);
            return true;
        }
        let radius = radius.max(half);
        let h = radius - (radius * radius - half * half).max(0.0).sqrt();
        let mut sign = 1.0;
        if self.hover_point.is_some_and(|hp| self.bulge_for(hp) < 0.0) {
            sign = -1.0;
        }
        let mut trim = None;
        if self.fillet.is_some() {
            if let Some(ht) = self.tangent_bulge(None, None) {
                if (ht.abs() - h).abs() <= 1e-6 * radius.max(1.0) {
                    sign = 1f64.copysign(ht);
                    self.bulge_kind = Some(BulgeKind::Fillet);
                    trim = self.corner_trim(vp, Modifiers::NONE);
                }
            }
        }
        let pts = self.points(None, Some(sign * h), None);
        if pts.len() >= 2 {
            self.commit(vp, &pts, trim);
        }
        true
    }

    /// "Ns": the segment count — of the arc being drawn, or of the one
    /// just drawn, rebuilt in place.
    pub fn on_segments_value(&mut self, vp: &mut Viewport, n: u32) -> bool {
        if n < 2 {
            return false;
        }
        self.segments = n as usize;
        if let (Some(mark), Some((apex, trim))) = (self.last_cmd, self.last_params.clone()) {
            if mark == (vp.history.undo_len(), vp.scene.version) {
                vp.undo();
                let pts2 = arc_3pts_2d((0.0, 0.0), apex.apex, apex.end, self.segments);
                let points: Vec<DVec3> = pts2
                    .into_iter()
                    .map(|(x, y)| apex.origin + apex.u * x + apex.v * y)
                    .collect();
                commit_arc(vp, &points, None, trim.clone());
                self.last_cmd = Some((vp.history.undo_len(), vp.scene.version));
                self.last_params = Some((apex, trim));
                vp.update();
            }
        }
        vp.flash_status(&tr("{n} segments", &[("n", &n)]), 2500);
        true
    }
}

/// The tangent leaving an arc's end vertex `p` within `mesh`.
fn tangent_in_mesh(mesh: &Mesh, p: DVec3) -> Option<DVec3> {
    for edge in mesh.edge_ids() {
        if mesh.edge(edge).curve.is_none() {
            continue;
        }
        let (a, b) = mesh.edge_ab(edge);
        if (a - p).length() > 1e-6 && (b - p).length() > 1e-6 {
            continue;
        }
        let chain = mesh.curve_edges(edge);
        let incident: Vec<EdgeId> = chain
            .iter()
            .copied()
            .filter(|&e| {
                let (a, b) = mesh.edge_ab(e);
                (a - p).length() <= 1e-6 || (b - p).length() <= 1e-6
            })
            .collect();
        if incident.len() != 1 {
            continue;
        }
        let (ea, eb) = mesh.edge_ab(incident[0]);
        let q = if (ea - p).length() <= 1e-6 { eb } else { ea };
        let mut pts: Vec<(VertexId, DVec3)> = Vec::new();
        for &c in &chain {
            let ed = mesh.edge(c);
            for vv in [ed.v0, ed.v1] {
                if !pts.iter().any(|x| x.0 == vv) {
                    pts.push((vv, mesh.pos(vv)));
                }
            }
        }
        let positions: Vec<DVec3> = pts.iter().map(|x| x.1).collect();
        let fit = if positions.len() >= 3 {
            snap::fit_circle(&positions, 0.01)
        } else {
            None
        };
        let Some((c, _r)) = fit else { continue };
        let radial = p - c;
        if radial.length() < 1e-9 {
            continue;
        }
        let r_hat = radial.unit_or_zero();
        let leaving = p - q;
        let t = leaving - r_hat * leaving.dot(r_hat);
        if t.length() < 1e-9 {
            continue;
        }
        return Some(t.unit_or_zero());
    }
    None
}

impl Tool for ArcTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("arc"),
            shortcut: Some("A"),
            description: Some("Draw an arc from its two ends, then pull out its bulge."),
            ..ToolInfo::new("arc", "Arc")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Bulge".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        self.wireframe
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.last_cmd = None;
        self.last_params = None;
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
        self.last_cmd = None;
        self.last_params = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        let Some(start) = self.start_point else {
            self.last_cmd = None;
            self.last_params = None;
            self.start_point = Some(ctx.world);
            if self.work_plane.is_none() {
                self.work_plane = self.lock.locked_work_plane(ctx.world);
            }
            self.start_edge = Self::edge_under(vp, ctx.world);
            return;
        };
        if self.end_point.is_none() {
            let end = self.equidistant.map_or(ctx.world, |e| e.1);
            if (end - start).length() < 1e-6 {
                return;
            }
            self.end_point = Some(end);
            let pts = self.plane_points();
            self.lock.adopt_snapped_plane(&mut self.work_plane, &pts);
            self.fillet = None;
            self.fillet_edge_b = None;
            if let Some((v, p2, b)) = self.equidistant {
                self.fillet = Some((v, start, p2));
                self.fillet_edge_b = Some(b);
            }
            self.equidistant = None;
            self.tangent_dir = self
                .tangent_at_start(vp)
                .or_else(|| self.edge_tangent(vp, end));
            return;
        }
        let pts = self.points(Some(ctx.world), None, None);
        if pts.len() >= 2 {
            let trim = self.corner_trim(vp, ctx.modifiers);
            self.commit(vp, &pts, trim);
        }
    }

    fn on_double_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.start_point.is_some() && self.end_point.is_some() && self.fillet.is_some() {
            if let Some(h) = self.tangent_bulge(None, None) {
                self.snap_bulge = Some(h);
                self.bulge_kind = Some(BulgeKind::Fillet);
                let pts = self.points(None, Some(h), None);
                if pts.len() >= 2 {
                    let trim = self.corner_trim(vp, ctx.modifiers);
                    self.commit(vp, &pts, trim);
                    return;
                }
            }
        }
        if self.start_point.is_some() && self.end_point.is_none() {
            if let Some(d) = last_fillet_d().filter(|d| *d != 0.0) {
                if let Some(corner) = Self::corner_near(vp, ctx.screen) {
                    self.reset();
                    self.round_corner(vp, corner, d, ctx.modifiers);
                    return;
                }
            }
        }
        self.on_click(vp, ctx);
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        self.snap_bulge = None;
        self.bulge_kind = None;
        self.equidistant = None;
        self.wireframe = self.lock.lock_color();
        if self.start_point.is_some() && self.end_point.is_none() {
            if self.start_edge.is_some() {
                self.wireframe = Some(rgba(COLOR_TANGENT));
                self.equidistant = self.equidistant_near(vp, ctx.screen);
                if let Some((_, p2, _)) = self.equidistant {
                    self.hover_point = Some(p2);
                    self.wireframe = Some(rgba(COLOR_FILLET));
                }
            }
        } else if self.end_point.is_some() {
            self.snap_bulge_for(vp, ctx);
        }
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            segments: true,
            radius: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        match *value {
            ToolValue::Segments(n) => return self.on_segments_value(vp, n),
            ToolValue::Radius(r) => return self.on_radius_value(vp, r),
            _ => {}
        }
        if self.end_point.is_none() || self.hover_point.is_none() {
            return false;
        }
        let Some(value) = value.as_num() else {
            return false;
        };
        if self.fillet.is_some() {
            // Both tangencies fixed: the number is the fillet's RADIUS.
            return self.fillet_with_radius(vp, value);
        }
        let sign = if self.bulge_for(self.hover_point.unwrap_or_default()) < 0.0 {
            -1.0
        } else {
            1.0
        };
        self.bulge_kind = None;
        let pts = self.points(None, Some(sign * value), None);
        if pts.len() >= 2 {
            self.commit(vp, &pts, None);
        }
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        if self.end_point.is_none() {
            if self.start_edge.is_some() {
                let tangent = self.edge_tangent(vp, hover);
                if let Some(h) = tangent.and_then(|t| self.tangent_bulge(Some(hover), Some(t))) {
                    let pts = self.points(None, Some(h), Some(hover));
                    if pts.len() >= 2 {
                        return pts.windows(2).map(|w| (w[0], w[1])).collect();
                    }
                }
            }
            return vec![(start, hover)];
        }
        let pts = self.points(Some(hover), None, None);
        pts.windows(2).map(|w| (w[0], w[1])).collect()
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return None;
        };
        let Some(end) = self.end_point else {
            if self.start_edge.is_some() {
                return Some((tr("Tangent to edge", &[]), Some((start + hover) * 0.5)));
            }
            return None;
        };
        let mid = (start + end) * 0.5;
        if let Some(sb) = self.snap_bulge {
            let label = match self.bulge_kind {
                Some(BulgeKind::Half) => tr("Half circle", &[]),
                Some(BulgeKind::Fillet) => tr("Tangent to edge", &[]),
                _ => tr("Tangent at vertex", &[]),
            };
            return Some((format!("{label}  {}", fmt_len(sb.abs())), Some(mid)));
        }
        let b = self.bulge_for(hover);
        Some((format!("Bulge {}", fmt_len(b.abs())), Some(mid)))
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        let started = self.start_point.is_some();
        self.lock.plane_lock_key(vp, key, started)
    }

    fn start_point(&self) -> Option<DVec3> {
        self.start_point
    }
    fn phase(&self) -> &'static str {
        if self.end_point.is_some() {
            "bulge"
        } else if self.start_point.is_some() {
            "end"
        } else {
            "idle"
        }
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

// ---- 3-point arc --------------------------------------------------------------------

/// The arc through three clicked points.
#[derive(Default)]
pub struct ThreePointArcTool {
    pub start_point: Option<DVec3>,
    pub mid_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
}

impl ThreePointArcTool {
    pub fn new() -> ThreePointArcTool {
        ThreePointArcTool::default()
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.mid_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
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
        match (self.start_point, self.mid_point) {
            (None, _) => Vec::new(),
            (Some(s), None) => std::iter::once(s).chain(self.hover_point).collect(),
            (Some(s), Some(m)) => [s, m].into_iter().chain(self.hover_point).collect(),
        }
    }

    fn axes(&self) -> (DVec3, DVec3) {
        plane_axes(
            self.lock
                .drawing_plane(self.work_plane, &self.plane_points())
                .1,
        )
    }

    pub fn points(&self, end: DVec3) -> Vec<DVec3> {
        let (Some(s), Some(m)) = (self.start_point, self.mid_point) else {
            return Vec::new();
        };
        let (u, v) = self.axes();
        let to2 = |p: DVec3| {
            let d = p - s;
            (d.dot(u), d.dot(v))
        };
        let (m2, e2) = (to2(m), to2(end));
        if e2.0.hypot(e2.1) < 1e-9 || m2.0.hypot(m2.1) < 1e-9 {
            return Vec::new();
        }
        arc_3pts_2d((0.0, 0.0), m2, e2, SEGMENTS)
            .into_iter()
            .map(|(x, y)| s + u * x + v * y)
            .collect()
    }
}

impl Tool for ThreePointArcTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("arc3"),
            shortcut: Some("J"),
            description: Some("Draw an arc that passes through three points."),
            ..ToolInfo::new("arc3", "3-Point Arc")
        };
        &INFO
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
        if self.mid_point.is_none() {
            if (ctx.world - start).length() < 1e-6 {
                return;
            }
            self.mid_point = Some(ctx.world);
            let pts = self.plane_points();
            self.lock.adopt_snapped_plane(&mut self.work_plane, &pts);
            return;
        }
        let pts = self.points(ctx.world);
        if pts.len() >= 2 {
            commit_arc(vp, &pts, None, None);
            self.reset();
            vp.update();
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(s), Some(h)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        if self.mid_point.is_none() {
            return vec![(s, h)];
        }
        self.points(h).windows(2).map(|w| (w[0], w[1])).collect()
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

// ---- Centre arc and Sector ----------------------------------------------------------

/// The 15° pitch of the 24-side circle.
const PITCH_DEG: f64 = 15.0;

/// Compass arc: centre → start (radius and 0° arm) → sweep. `sector` closes
/// the wedge with its two radii.
pub struct CenterArcTool {
    pub sector: bool,
    pub start_point: Option<DVec3>,
    pub arm_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    pub lock: PlaneLock,
}

impl CenterArcTool {
    pub fn new() -> CenterArcTool {
        CenterArcTool {
            sector: false,
            start_point: None,
            arm_point: None,
            hover_point: None,
            work_plane: None,
            lock: PlaneLock::default(),
        }
    }

    /// The Sector: the centre arc whose wedge closes into a face.
    pub fn sector() -> CenterArcTool {
        CenterArcTool {
            sector: true,
            ..CenterArcTool::new()
        }
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.arm_point = None;
        self.work_plane = None;
        self.lock.hover_plane = None;
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
            Some(s) => std::iter::once(s)
                .chain(self.arm_point.or(self.hover_point))
                .collect(),
        }
    }

    fn axes(&self) -> (DVec3, DVec3) {
        plane_axes(
            self.lock
                .drawing_plane(self.work_plane, &self.plane_points())
                .1,
        )
    }

    /// Signed sweep (degrees) from the 0° arm to `cursor`.
    pub fn sweep_to(&self, cursor: DVec3) -> f64 {
        let (Some(c), Some(arm)) = (self.start_point, self.arm_point) else {
            return 0.0;
        };
        let (u, v) = self.axes();
        let (a, b) = (arm - c, cursor - c);
        let a0 = a.dot(v).atan2(a.dot(u));
        let b0 = b.dot(v).atan2(b.dot(u));
        let mut deg = (b0 - a0).to_degrees();
        while deg <= -180.0 {
            deg += 360.0;
        }
        while deg > 180.0 {
            deg -= 360.0;
        }
        deg
    }

    pub fn points(&self, sweep_deg: f64) -> Vec<DVec3> {
        if sweep_deg.abs() < 1e-6 {
            return Vec::new();
        }
        let (Some(c), Some(arm)) = (self.start_point, self.arm_point) else {
            return Vec::new();
        };
        let (u, v) = self.axes();
        let a = arm - c;
        let r = a.dot(u).hypot(a.dot(v));
        if r < 1e-6 {
            return Vec::new();
        }
        let a0 = a.dot(v).atan2(a.dot(u));
        let steps =
            (xtrude_core::math::round_even_int(sweep_deg.abs() / PITCH_DEG)).max(1) as usize;
        (0..=steps)
            .map(|k| {
                let t = a0 + sweep_deg.to_radians() * k as f64 / steps as f64;
                c + (u * t.cos() + v * t.sin()) * r
            })
            .collect()
    }

    fn commit(&mut self, vp: &mut Viewport, pts: &[DVec3]) {
        let close = if self.sector { self.start_point } else { None };
        commit_arc(vp, pts, close, None);
        self.reset();
        vp.update();
    }
}

impl Default for CenterArcTool {
    fn default() -> Self {
        CenterArcTool::new()
    }
}

impl Tool for CenterArcTool {
    fn info(&self) -> &'static ToolInfo {
        static CENTER_ARC: ToolInfo = ToolInfo {
            icon: Some("center_arc"),
            shortcut: Some("Shift+O"),
            description: Some(
                "Draw an arc from its centre: the centre, where the arc starts, then the angle it sweeps.",
            ),
            ..ToolInfo::new("center_arc", "Center Arc")
        };
        static SECTOR: ToolInfo = ToolInfo {
            icon: Some("sector"),
            description: Some(
                "Draw an arc from its centre whose two radii close it into a slice-shaped face.",
            ),
            ..ToolInfo::new("sector", "Sector")
        };
        if self.sector {
            &SECTOR
        } else {
            &CENTER_ARC
        }
    }
    fn value_box_caption(&self) -> Option<String> {
        Some(
            if self.arm_point.is_some() {
                "Angle"
            } else {
                "Radius"
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
        self.lock.note_plane(vp);
        let Some(start) = self.start_point else {
            self.start_point = Some(ctx.world);
            if self.work_plane.is_none() {
                self.work_plane = self.lock.locked_work_plane(ctx.world);
            }
            return;
        };
        if self.arm_point.is_none() {
            if (ctx.world - start).length() < 1e-6 {
                return;
            }
            self.arm_point = Some(ctx.world);
            let pts = self.plane_points();
            self.lock.adopt_snapped_plane(&mut self.work_plane, &pts);
            return;
        }
        let pts = self.points(self.sweep_to(ctx.world));
        if pts.len() >= 2 {
            self.commit(vp, &pts);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.lock.note_plane(vp);
        self.hover_point = Some(ctx.world);
        vp.update();
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if self.arm_point.is_none() {
            return false;
        }
        let Some(value) = value.as_num() else {
            return false;
        };
        let mut sign = -1.0;
        if let Some(h) = self.hover_point {
            sign = if self.sweep_to(h) < 0.0 { -1.0 } else { 1.0 };
        }
        let pts = self.points(sign * value.abs());
        if pts.len() >= 2 {
            self.commit(vp, &pts);
        }
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(c), Some(h)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        let Some(arm) = self.arm_point else {
            return vec![(c, h)];
        };
        let mut segs = vec![(c, arm), (c, h)];
        let pts = self.points(self.sweep_to(h));
        segs.extend(pts.windows(2).map(|w| (w[0], w[1])));
        segs
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (Some(c), Some(h)) = (self.start_point, self.hover_point) else {
            return None;
        };
        if self.arm_point.is_none() {
            let (u, v) = self.axes();
            let d = h - c;
            return Some((format!("R {}", fmt_len(d.dot(u).hypot(d.dot(v)))), Some(h)));
        }
        Some((format!("{:+.1}°", self.sweep_to(h)), Some(h)))
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
