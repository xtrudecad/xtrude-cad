// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Push/Pull: extrude a face along its normal — the geometric pipeline
//! (the interaction lives in the app's tool).
//!
//! Commit pipeline (the deterministic root-fix, no case tree):
//! 1. Orient the mesh outward (entry invariant; no-op on open sheets).
//! 2. Build the *naive* extrusion: consumed/kept base, moved cap (with
//!    holes), one wall quad per boundary/hole edge.
//! 3. Stitch connectivity (T-junctions, collinear vertices).
//! 4. On a solid: deterministically rebuild every touched plane from its
//!    edges (`cap_rebuild`); on a flat sheet, the seeded coplanar merge.
//! 5. Orient outward again.
//!
//! A bordered base face is consumed either direction — pushing in carves a
//! recess / step / through-hole; pushing out grows the solid. A free-standing
//! face keeps its base as the cap of the new box. A push on a closed solid
//! that would leave it open is **refused** (the hermeticity guard).
//!
//! Design note: the pipeline runs on a mesh id, so it can target a group's
//! mesh directly.

use std::collections::HashSet;

use xtrude_core::arrangement::{
    interior_point as interior_point_2d, planar_arrangement, plane_basis, point_in_polygon,
    scan_interior_point,
};
use xtrude_core::attrs::{FaceAttrs, Paint};
use xtrude_core::cap_rebuild::{
    apply_rebuild, crack_planes, edges_along, plane_key, seam_planes, RebuildCache, RebuildOpts,
};
use xtrude_core::geometry::PolyFace;
use xtrude_core::math::{DVec3, VecExt};
use xtrude_core::mesh::{FaceId, Key, Mesh, VertexId};
use xtrude_core::orient::{is_closed, orient_outward, ray_triangle, DEFAULT_SEED};
use xtrude_core::topology::{
    classify_push_edge, heal_overlapping_faces, loop_inside_face, mesh_is_flat,
    point_in_face_solid, refine_loop_with_points, subtract_loop_from_face, tkey, PushEdgeKind,
    SPLIT_TOLERANCE,
};

use crate::commands::mesh::{
    add_edge_stamped, add_face_auto, delete_face_by_loop, prune_orphans_at, run_stitch,
};
use crate::history::{CmdResult, FnCommand, History};
use crate::ids::MeshId;
use crate::scene::Scene;

/// The smallest extrusion that still moves geometry.
pub const MIN_EXTRUDE: f64 = 2e-4;

/// The dihedral above which a swept seam reads as a *curve* facet.
pub const CURVE_FACET_COS: f64 = 0.85;

/// How many halvings the commit tries when the guard refuses.
const CLAMP_TRIES: usize = 6;

/// How far into the base `PushPull::wall_cut` looks for which side of a
/// wall the push sweeps.
const WALL_SIDE_PROBE: f64 = 1e-3;

fn project_loop_2d(lp: &[DVec3], normal: DVec3) -> Vec<(f64, f64)> {
    let (u, v) = plane_basis(normal);
    lp.iter().map(|p| (p.dot(u), p.dot(v))).collect()
}

fn loops_overlap_2d(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    if point_in_polygon(interior_point_2d(a), b) || point_in_polygon(interior_point_2d(b), a) {
        return true;
    }
    a.iter().any(|&p| point_in_polygon(p, b)) || b.iter().any(|&p| point_in_polygon(p, a))
}

/// Whether `face`'s edge-connected component is watertight.
pub fn component_is_closed(mesh: &Mesh, face: FaceId) -> bool {
    let mut seen: HashSet<FaceId> = HashSet::from([face]);
    let mut stack = vec![face];
    while let Some(f) = stack.pop() {
        for lp in mesh.face(f).loops() {
            let n = lp.len();
            for i in 0..n {
                let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) else {
                    continue;
                };
                let ef = &mesh.edge(e).faces;
                if ef.len() < 2 {
                    return false;
                }
                for &g in ef {
                    if seen.insert(g) {
                        stack.push(g);
                    }
                }
            }
        }
    }
    true
}

/// Whether `neighbour` has an edge parallel to the push (a prism wall that
/// a translation slides along its own length).
pub fn swept_by_push(mesh: &Mesh, neighbour: FaceId, push_normal: DVec3) -> bool {
    let n = push_normal.unit_or_zero();
    let verts = mesh.face_positions(neighbour);
    let m = verts.len();
    (0..m).any(|i| {
        let e = verts[(i + 1) % m] - verts[i];
        e.length() > 1e-9 && e.unit_or_zero().dot(n).abs() > 0.999
    })
}

/// The base face's paint, ready to stamp on the geometry a push creates.
pub fn paint_of(attrs: &FaceAttrs) -> Paint {
    attrs.paint.clone()
}

/// `(origin, normal)` of each plane where an edge along `segments` parts
/// two coplanar faces facing the same way with the same paint.
fn seam_planes_along(mesh: &Mesh, segments: &[(DVec3, DVec3)]) -> Vec<(DVec3, DVec3)> {
    let mut out = Vec::new();
    for e in edges_along(mesh, segments) {
        let ef = &mesh.edge(e).faces;
        if ef.len() != 2 {
            continue;
        }
        let (fa, fb) = (ef[0], ef[1]);
        let (a, b) = (mesh.face(fa), mesh.face(fb));
        if a.interior
            || b.interior
            || a.attrs != b.attrs
            || mesh
                .face_normal(fa)
                .unit_or_zero()
                .dot(mesh.face_normal(fb).unit_or_zero())
                < 0.9999
        {
            continue;
        }
        out.push((mesh.face_centroid(fa), mesh.face_normal(fa)));
    }
    out
}

/// The outcome of a commit, for the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// Nothing to do (no face / zero distance).
    NoOp,
    Committed,
    /// The guard refused the dragged distance; the furthest accepted one
    /// was committed instead.
    ToppedOut,
    /// The guard refused the push; the solid was left unchanged.
    Refused,
}

/// A face ring of the naive sweep: `(base_ring, moved_ring)`.
pub type SweepRing = (Vec<DVec3>, Vec<DVec3>);

/// The walls a push into the solid cuts down (`PushPull::wall_cut`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallCut {
    /// The walls the push trims.
    pub walls: Vec<FaceId>,
    /// What stays of the walls, as faces wearing their wall's paint.
    pub remains: Vec<PolyFace>,
    /// The sweep's sides that run down a cut wall, as `(ring, edge)` of
    /// `light_faces`' rings: the strip each sweeps is what comes off.
    pub sides: Vec<(usize, usize)>,
}

/// One side's strip that a wall gives up: `(ring, edge)` and its quad.
struct Strip {
    side: (usize, usize),
    quad: [DVec3; 4],
}

/// The drag state of one push/pull.
#[derive(Clone, Debug, Default)]
pub struct PushPull {
    /// The face being pushed, in `target`.
    pub base_face: Option<FaceId>,
    /// The mesh the push edits (the active mesh, or a group's).
    pub target: Option<MeshId>,
    /// Signed distance along `normal`.
    pub extrusion: f64,
    /// Ctrl: push/pull a copy (the base stays as a dividing face).
    pub keep_base: bool,
    /// Deepest allowed inward push (positive), `None` = unbounded.
    pub limit_in: Option<f64>,
    /// The base is embedded (every edge shared).
    pub attached: bool,
    /// Every edge backs onto a perpendicular swept wall (a clean prism).
    pub prism_cap: bool,
    pub drag_pre_oriented: bool,
    /// Fixed centroid the extrusion is measured from.
    pub anchor: Option<DVec3>,
    pub normal: Option<DVec3>,
    /// Every boundary position of the base (outer + holes).
    pub cap_positions: Vec<DVec3>,
    /// The cap's vertices, resolved once (the prism preview moves them).
    pub prism_verts: Vec<VertexId>,
    /// How much of `extrusion` the prism preview has applied.
    pub prism_applied: f64,
    pub refused: bool,
    pub topped_out: bool,
    /// Test hook: replaces the inner mutation, to prove the guard.
    #[doc(hidden)]
    pub inner_override: Option<fn(&mut Mesh)>,
}

impl PushPull {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock onto `face` of mesh `target` (the drag-start click).
    pub fn begin(&mut self, scene: &mut Scene, target: MeshId, face: FaceId) {
        self.reset();
        self.base_face = Some(face);
        self.target = Some(target);
        {
            let mesh = scene.doc.mesh_mut(target);
            orient_outward(mesh, DEFAULT_SEED, Some(&[face]));
        }
        self.drag_pre_oriented = true;
        let mesh = scene.doc.mesh(target);
        self.anchor = Some(mesh.face_centroid(face));
        self.normal = Some(outward_normal(mesh, face));
        let (attached, prism) = self.classify_base(mesh);
        self.attached = attached;
        self.prism_cap = prism;
        self.cap_positions = cap_loop_positions(mesh, face);
        let keys: HashSet<Key> = self.cap_positions.iter().map(|&p| tkey(p)).collect();
        self.prism_verts = mesh
            .vertex_ids()
            .into_iter()
            .filter(|&v| keys.contains(&tkey(mesh.pos(v))))
            .collect();
        self.compute_inward_limit(mesh);
    }

    /// Set up from a face the way the fuzz bench does (no orientation
    /// pass; the face's own normal).
    pub fn setup_plain(&mut self, mesh: &Mesh, target: MeshId, face: FaceId) {
        self.reset();
        self.base_face = Some(face);
        self.target = Some(target);
        self.anchor = Some(mesh.face_centroid(face));
        self.normal = Some(mesh.face_normal(face));
        let (attached, prism) = self.classify_base(mesh);
        self.attached = attached;
        self.prism_cap = prism;
        self.cap_positions = cap_loop_positions(mesh, face);
        self.compute_inward_limit(mesh);
    }

    pub fn dragging(&self) -> bool {
        self.base_face.is_some()
    }

    pub fn reset(&mut self) {
        *self = PushPull::default();
    }

    /// `(attached, prism_cap)` for previewing.
    pub fn classify_base(&self, mesh: &Mesh) -> (bool, bool) {
        let Some(face) = self.base_face else {
            return (false, false);
        };
        let faces = mesh.face_ids();
        let mut kinds = Vec::new();
        let mut loops = vec![mesh.face_positions(face)];
        loops.extend(mesh.face_hole_positions(face));
        for lp in &loops {
            let n = lp.len();
            for i in 0..n {
                kinds.push(classify_push_edge(
                    mesh,
                    face,
                    lp[i],
                    lp[(i + 1) % n],
                    &faces,
                ));
            }
        }
        let normal = mesh.face_normal(face);
        let attached = kinds.iter().all(|(k, _)| *k != PushEdgeKind::Free);
        let prism = !kinds.is_empty()
            && kinds.iter().all(|(k, nb)| {
                *k == PushEdgeKind::Perp && nb.is_some_and(|nb| swept_by_push(mesh, nb, normal))
            });
        (attached, prism)
    }

    /// How far the face can be pushed *into* the solid (the "Push limited
    /// to" rule).
    pub fn compute_inward_limit(&mut self, mesh: &Mesh) {
        self.limit_in = None;
        let Some(base) = self.base_face else { return };
        if !self.attached {
            return;
        }
        let n = self
            .normal
            .unwrap_or_else(|| mesh.face_normal(base))
            .unit_or_zero();
        let anchor = self.anchor.unwrap_or_else(|| mesh.face_centroid(base));
        let base_pos = mesh.face_positions(base);
        let base_xy = project_loop_2d(&base_pos, n);
        for g in mesh.face_ids() {
            if g == base {
                continue;
            }
            let gn = mesh.face_normal(g).unit_or_zero();
            if gn.dot(n).abs() < 0.999 {
                continue;
            }
            let dist = (anchor - mesh.face_centroid(g)).dot(n);
            if dist <= 1e-6 {
                continue;
            }
            let back: Vec<DVec3> = base_pos.iter().map(|&v| v - n * dist).collect();
            let gp = mesh.poly_face(g);
            if loop_inside_face(&gp, &back, false) {
                continue; // a through-hole target
            }
            if !loops_overlap_2d(&base_xy, &project_loop_2d(&gp.vertices, n)) {
                continue;
            }
            if self.limit_in.is_none_or(|l| dist < l) {
                self.limit_in = Some(dist);
            }
        }
        let direction = -n;
        for lp in mesh.face(base).loops() {
            let cnt = lp.len();
            for i in 0..cnt {
                let Some(ed) = mesh.find_edge(lp[i], lp[(i + 1) % cnt]) else {
                    continue;
                };
                for &g in &mesh.edge(ed).faces {
                    if g == base || mesh.face(g).interior {
                        continue;
                    }
                    let gn = mesh.face_normal(g).unit_or_zero();
                    if gn.dot(n).abs() > 0.999 {
                        continue;
                    }
                    if direction.dot(gn) > 1e-6 {
                        self.limit_in = Some(0.0);
                        return;
                    }
                }
            }
        }
        let (u, w) = plane_basis(n);
        let origin3 = base_pos[0];
        let loop_xy: Vec<(f64, f64)> = base_pos
            .iter()
            .map(|&p| ((p - origin3).dot(u), (p - origin3).dot(w)))
            .collect();
        let ip = interior_point_2d(&loop_xy);
        let ip3 = origin3 + u * ip.0 + w * ip.1;
        let mut samples = vec![ip3];
        for (i, &p) in base_pos.iter().enumerate() {
            samples.push(p + (ip3 - p) * 1e-3);
            let mid = (p + base_pos[(i + 1) % base_pos.len()]) * 0.5;
            samples.push(mid + (ip3 - mid) * 1e-3);
        }
        let mut lateral: Option<f64> = None;
        for g in mesh.face_ids() {
            if g == base || mesh.face(g).interior {
                continue;
            }
            if mesh.face_normal(g).unit_or_zero().dot(n).abs() > 0.999 {
                continue;
            }
            for tri in mesh.face_triangulate(g, None) {
                for &q in &samples {
                    if let Some(t) = ray_triangle(q, direction, &tri) {
                        if t > 1e-4 && lateral.is_none_or(|l| t < l) {
                            lateral = Some(t);
                        }
                    }
                }
            }
        }
        if let Some(l) = lateral {
            let l = (l - 1e-3).max(0.0);
            if self.limit_in.is_none_or(|li| l < li) {
                self.limit_in = Some(l);
            }
        }
    }

    /// Clamp the extrusion to the inward limit; returns the limit when the
    /// clamp engaged (the tool flashes "Push limited to …").
    pub fn clamp_extrusion(&mut self) -> Option<f64> {
        if let Some(l) = self.limit_in {
            if self.extrusion < -l {
                self.extrusion = -l;
                return Some(l);
            }
        }
        None
    }

    // ---- Preview geometry -------------------------------------------------------

    /// The naive sweep as preview polygons (moved cap + one quad per edge)
    /// and its rings.
    pub fn light_faces(&self, mesh: &Mesh) -> (Vec<PolyFace>, Vec<SweepRing>) {
        let (Some(face), Some(n)) = (self.base_face, self.normal) else {
            return (Vec::new(), Vec::new());
        };
        if !mesh.has_face(face) || self.extrusion.abs() < MIN_EXTRUDE {
            return (Vec::new(), Vec::new());
        }
        let off = n * self.extrusion;
        let attrs = mesh.face(face).attrs.clone();
        let wall = paint_of(&attrs);
        let wall_attrs = (!wall.is_empty()).then(|| FaceAttrs {
            paint: wall,
            ..FaceAttrs::default()
        });
        let mut rings = vec![{
            let v = mesh.face_positions(face);
            let hi: Vec<DVec3> = v.iter().map(|&p| p + off).collect();
            (v, hi)
        }];
        for h in mesh.face_hole_positions(face) {
            let hi: Vec<DVec3> = h.iter().map(|&p| p + off).collect();
            rings.push((h, hi));
        }
        let mut out = vec![PolyFace {
            vertices: rings[0].1.clone(),
            holes: rings[1..].iter().map(|r| r.1.clone()).collect(),
            attrs: (!attrs.is_empty()).then_some(attrs),
        }];
        for (low, high) in &rings {
            let c = low.len();
            for i in 0..c {
                let j = (i + 1) % c;
                out.push(PolyFace {
                    vertices: vec![low[i], low[j], high[j], high[i]],
                    holes: Vec::new(),
                    attrs: wall_attrs.clone(),
                });
            }
        }
        (out, rings)
    }

    /// What a push into the solid cuts off the walls it slides down: every
    /// wall that carries a base edge, runs along the push and has the swept
    /// volume behind it loses the strip that edge sweeps. Empty for a pull,
    /// a kept base, or a mesh that is not a closed solid.
    pub fn wall_cut(&self, mesh: &Mesh) -> WallCut {
        let mut cut = WallCut::default();
        let (Some(face), Some(n)) = (self.base_face, self.normal) else {
            return cut;
        };
        if self.keep_base
            || self.extrusion > -MIN_EXTRUDE
            || !mesh.has_face(face)
            || !is_closed(mesh)
        {
            return cut;
        }
        let n = n.unit_or_zero();
        let off = n * self.extrusion;
        let faces = mesh.face_ids();
        let base = mesh.poly_face(face);
        let mut loops = vec![mesh.face_positions(face)];
        loops.extend(mesh.face_hole_positions(face));
        // Each wall with the strips it gives up, in order.
        let mut strips: Vec<(FaceId, Vec<Strip>)> = Vec::new();
        for (r, lp) in loops.iter().enumerate() {
            let c = lp.len();
            for i in 0..c {
                let (a, b) = (lp[i], lp[(i + 1) % c]);
                let (PushEdgeKind::Perp, Some(w)) = classify_push_edge(mesh, face, a, b, &faces)
                else {
                    continue;
                };
                let wn = mesh.face_normal(w).unit_or_zero();
                if mesh.face(w).interior || wn.dot(n).abs() > 1e-4 {
                    continue;
                }
                // The swept volume lies on the base's side of the edge: the
                // wall loses its strip only when that is behind it.
                let mid = (a + b) * 0.5;
                if !point_in_face_solid(&base, mid - wn * WALL_SIDE_PROBE) {
                    continue;
                }
                let strip = Strip {
                    side: (r, i),
                    quad: [a, b, b + off, a + off],
                };
                match strips.iter_mut().find(|(g, _)| *g == w) {
                    Some((_, sides)) => sides.push(strip),
                    None => strips.push((w, vec![strip])),
                }
            }
        }
        for (w, sides) in strips {
            let wall = mesh.poly_face(w);
            let wn = mesh.face_normal(w).unit_or_zero();
            let quads: Vec<PolyFace> = sides
                .iter()
                .map(|st| PolyFace::new(st.quad.to_vec()))
                .collect();
            let mut segments = Vec::new();
            for lp in std::iter::once(&wall.vertices)
                .chain(&wall.holes)
                .chain(quads.iter().map(|q| &q.vertices))
            {
                let c = lp.len();
                segments.extend((0..c).map(|i| (lp[i], lp[(i + 1) % c])));
            }
            let (u, v) = plane_basis(wn);
            let o = wall.vertices[0];
            let to2 = |lp: &[DVec3]| -> Vec<(f64, f64)> {
                lp.iter()
                    .map(|&p| ((p - o).dot(u), (p - o).dot(v)))
                    .collect()
            };
            let attrs = &mesh.face(w).attrs;
            let attrs = (!attrs.is_empty()).then(|| attrs.clone());
            let mut remains = Vec::new();
            for (outer, holes) in planar_arrangement(&segments, o, wn) {
                let holes2: Vec<Vec<(f64, f64)>> = holes.iter().map(|h| to2(h)).collect();
                let Some((x, y)) = scan_interior_point(&to2(&outer), &holes2) else {
                    continue;
                };
                let p = o + u * x + v * y;
                if point_in_face_solid(&wall, p) && !quads.iter().any(|q| point_in_face_solid(q, p))
                {
                    remains.push(PolyFace {
                        vertices: outer,
                        holes,
                        attrs: attrs.clone(),
                    });
                }
            }
            if remains.is_empty() {
                continue; // the push takes the whole wall: its sweep stands for it
            }
            cut.walls.push(w);
            cut.remains.extend(remains);
            cut.sides.extend(sides.iter().map(|st| st.side));
        }
        cut
    }

    /// The sweep's edges: moved ring + risers (skipping risers the commit
    /// will soften). The base ring is not drawn — the model draws it.
    pub fn rubber_band_lines(rings: &[SweepRing]) -> Vec<(DVec3, DVec3)> {
        Self::rubber_band_lines_cut(rings, &[])
    }

    /// `rubber_band_lines` of a push that cuts walls: a riser between two
    /// cut `sides` (`WallCut::sides`) is the part of a corner the push
    /// removes, so it is left out.
    pub fn rubber_band_lines_cut(
        rings: &[SweepRing],
        sides: &[(usize, usize)],
    ) -> Vec<(DVec3, DVec3)> {
        let mut segs = Vec::new();
        for (r, (low, high)) in rings.iter().enumerate() {
            let n = low.len();
            if n < 2 {
                continue;
            }
            let off = high[0] - low[0];
            let normals: Vec<Option<DVec3>> = (0..n)
                .map(|i| {
                    let nv = (low[(i + 1) % n] - low[i]).cross(off);
                    (nv.length() > 1e-12).then(|| nv.unit_or_zero())
                })
                .collect();
            for i in 0..n {
                let j = (i + 1) % n;
                segs.push((high[i], high[j]));
                let a = normals[(i + n - 1) % n];
                let b = normals[i];
                if let (Some(a), Some(b)) = (a, b) {
                    let d = a.dot(b);
                    if CURVE_FACET_COS < d && d < 0.99995 {
                        continue;
                    }
                }
                if sides.contains(&(r, (i + n - 1) % n)) && sides.contains(&(r, i)) {
                    continue;
                }
                segs.push((low[i], high[i]));
            }
        }
        segs
    }

    /// Slide the cap's own vertices by `delta` along the normal (the prism
    /// preview), booking `applied` as the total.
    pub fn move_cap(&mut self, scene: &mut Scene, delta: f64, applied: f64) {
        let (Some(t), Some(n)) = (self.target, self.normal) else {
            return;
        };
        let step = n * delta;
        let mesh = scene.doc.mesh_mut(t);
        for &v in &self.prism_verts {
            if mesh.has_vertex(v) {
                mesh.move_vertex(v, step);
            }
        }
        self.prism_applied = applied;
        scene.version += 1;
    }

    /// Put a translated cap back where it started.
    pub fn revert_prism_preview(&mut self, scene: &mut Scene) {
        if self.prism_applied != 0.0 {
            let a = self.prism_applied;
            self.move_cap(scene, -a, 0.0);
        }
    }

    /// Preview a prism extend/shrink by translating the cap in the model.
    pub fn show_prism_preview(&mut self, scene: &mut Scene) {
        let delta = self.extrusion - self.prism_applied;
        if delta.abs() > 1e-12 {
            let ext = self.extrusion;
            self.move_cap(scene, delta, ext);
        }
    }

    // ---- Commit ----------------------------------------------------------------------

    /// Commit the push as one undo step. Returns the outcome; the caller
    /// resets the drag.
    pub fn commit(&mut self, scene: &mut Scene, history: &mut History) -> PushOutcome {
        self.revert_prism_preview(scene);
        let (Some(_), Some(_)) = (self.base_face, self.target) else {
            return PushOutcome::NoOp;
        };
        if self.extrusion.abs() < MIN_EXTRUDE {
            return PushOutcome::NoOp;
        }
        self.topped_out = false;
        let mut me = self.clone();
        let mut cmd = FnCommand::new("SnapshotMutation", |sc: &mut Scene| {
            me.mutate_or_top_out(sc)
        });
        history.execute(scene, &mut cmd);
        drop(cmd);
        self.extrusion = me.extrusion;
        self.refused = me.refused;
        self.topped_out = me.topped_out;
        self.outcome()
    }

    /// What the last commit or [`push_face_in_scene`] did.
    pub fn outcome(&self) -> PushOutcome {
        if self.topped_out {
            PushOutcome::ToppedOut
        } else if self.refused {
            PushOutcome::Refused
        } else {
            PushOutcome::Committed
        }
    }

    /// Commit; if the guard refuses the dragged distance, commit the
    /// furthest one it accepts instead (bisection).
    pub fn mutate_or_top_out(&mut self, scene: &mut Scene) -> CmdResult {
        self.mutate(scene);
        if !self.refused {
            return Ok(());
        }
        let Some(t) = self.target else { return Ok(()) };
        let wanted = self.extrusion;
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        let mut best = None;
        for _ in 0..CLAMP_TRIES {
            let mid = (lo + hi) / 2.0;
            let snap = scene.doc.mesh(t).clone();
            self.extrusion = wanted * mid;
            self.mutate(scene);
            let ok = !(self.refused || self.extrusion.abs() < MIN_EXTRUDE);
            if ok {
                best = Some(self.extrusion);
            }
            *scene.doc.mesh_mut(t) = snap;
            if ok {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        match best {
            None => {
                self.extrusion = wanted;
                self.refused = true;
            }
            Some(b) => {
                self.extrusion = b;
                self.mutate(scene);
                self.topped_out = true;
            }
        }
        Ok(())
    }

    /// Apply the push and stitch watertight, with the **watertight refusal
    /// guard**.
    pub fn mutate(&mut self, scene: &mut Scene) {
        self.refused = false;
        let Some(t) = self.target else { return };
        if self.mutate_mesh(scene.doc.mesh_mut(t)) {
            scene.version += 1;
        }
    }

    /// Whether a commit would push exactly `extrusion`, neither refused
    /// nor topped out: the push tried on a copy of `mesh`, which must be
    /// the mesh as it is without the prism preview.
    pub fn accepts(&self, mesh: &Mesh) -> bool {
        if self.base_face.is_none() || self.extrusion.abs() < MIN_EXTRUDE {
            return false;
        }
        let mut trial = self.clone();
        let mut copy = mesh.clone();
        trial.refused = false;
        trial.mutate_mesh(&mut copy);
        !trial.refused
    }

    /// [`Self::mutate`] on the mesh itself: whether it changed (`false`
    /// when refused, the mesh left as it was).
    fn mutate_mesh(&mut self, mesh: &mut Mesh) -> bool {
        let before_edges: HashSet<xtrude_core::EdgeId> = mesh.edge_ids().into_iter().collect();
        let base_paint = self
            .base_face
            .filter(|&f| mesh.has_face(f))
            .map(|f| paint_of(&mesh.face(f).attrs))
            .filter(|p| !p.is_empty());
        let before_verts: HashSet<VertexId> = mesh.vertex_ids().into_iter().collect();
        let guard = (is_closed(mesh) && !mesh_is_flat(mesh)).then(|| mesh.clone());
        let fp_before = guard.as_ref().map(mesh_fingerprint);
        match self.inner_override {
            Some(f) => f(mesh),
            None => self.mutate_inner(mesh),
        }
        if let Some(g) = guard {
            if !(is_closed(mesh) || mesh_is_flat(mesh)) {
                *mesh = g;
                self.refused = true;
                return false;
            }
        }
        if let Some(fp) = fp_before {
            if self.extrusion.abs() >= MIN_EXTRUDE && mesh_fingerprint(mesh) == fp {
                self.refused = true;
                return false;
            }
        }
        soften_curve_facets(mesh, &before_edges);
        if let Some(paint) = base_paint {
            let mut fresh: HashSet<FaceId> = HashSet::new();
            for v in mesh.vertex_ids() {
                if !before_verts.contains(&v) {
                    fresh.extend(mesh.vertex_faces(v));
                }
            }
            for f in fresh {
                if mesh.has_face(f) && mesh.face(f).attrs.paint.is_empty() {
                    let a = &mut mesh.face_mut(f).attrs;
                    a.paint = paint.clone();
                }
            }
        }
        true
    }

    fn mutate_inner(&mut self, mesh: &mut Mesh) {
        let Some(face) = self.base_face else { return };
        if !mesh.has_face(face) {
            return;
        }
        let mut d = self.extrusion;
        if self.prism_cap && !self.keep_base {
            let normal = self.normal.unwrap_or_else(|| mesh.face_normal(face));
            let keys: HashSet<Key> = self.cap_positions.iter().map(|&p| tkey(p)).collect();
            crate::commands::mesh::translate_points(mesh, &keys, normal * d);
            let mut seed = self.cap_positions.clone();
            seed.extend(self.cap_positions.iter().map(|&p| p + normal * d));
            let seedkeys: HashSet<Key> = seed.iter().map(|&p| tkey(p)).collect();
            run_stitch(mesh, &seed, Some(&mut HashSet::new()), false, true);
            if !mesh_is_flat(mesh) {
                let fresh: HashSet<FaceId> = mesh
                    .face_ids()
                    .into_iter()
                    .filter(|&f| {
                        mesh.face_positions(f)
                            .iter()
                            .any(|&p| seedkeys.contains(&tkey(p)))
                    })
                    .collect();
                rebuild_planes_fixpoint(mesh, fresh, &seed, false, d < 0.0);
            } else {
                heal_overlapping_faces(mesh, 0.5, None, None);
            }
            let touched = touched_faces(mesh, &seedkeys);
            orient_outward(mesh, DEFAULT_SEED, touched.as_deref());
            return;
        }
        let entry_closed = is_closed(mesh);
        let pre_normal = self.normal.unwrap_or_else(|| mesh.face_normal(face));
        if entry_closed && !self.drag_pre_oriented {
            orient_outward(mesh, DEFAULT_SEED, Some(&[face]));
        }
        let normal = mesh.face_normal(face);
        if normal.dot(pre_normal) < 0.0 {
            d = -d;
        }
        let allverts: Vec<DVec3> = mesh.verts.iter().map(|(_, v)| v.position).collect();
        let base = refine_loop_with_points(&mesh.face_positions(face), &allverts, SPLIT_TOLERANCE);
        let top: Vec<DVec3> = base.iter().map(|&v| v + normal * d).collect();
        let base_holes: Vec<Vec<DVec3>> = mesh
            .face_hole_positions(face)
            .iter()
            .map(|h| refine_loop_with_points(h, &allverts, SPLIT_TOLERANCE))
            .collect();
        let top_holes: Vec<Vec<DVec3>> = base_holes
            .iter()
            .map(|h| h.iter().map(|&v| v + normal * d).collect())
            .collect();
        let count = base.len();
        let faces = mesh.face_ids();
        let kinds: Vec<(PushEdgeKind, Option<FaceId>)> = (0..count)
            .map(|i| classify_push_edge(mesh, face, base[i], base[(i + 1) % count], &faces))
            .collect();
        let attached_any = kinds.iter().all(|(k, _)| *k != PushEdgeKind::Free);
        let attached = attached_any && !self.keep_base;
        let through = if attached {
            find_through_face(mesh, face, d)
        } else {
            None
        };
        let mut sheet_fast =
            !entry_closed && kinds.iter().all(|(k, _)| *k == PushEdgeKind::Coplanar);
        if sheet_fast {
            'h: for h in &base_holes {
                let m = h.len();
                for i in 0..m {
                    if classify_push_edge(mesh, face, h[i], h[(i + 1) % m], &faces).0
                        != PushEdgeKind::Coplanar
                    {
                        sheet_fast = false;
                        break 'h;
                    }
                }
            }
        }
        if sheet_fast && component_is_closed(mesh, face) {
            sheet_fast = false;
        }
        let was_solid = !mesh_is_flat(mesh) && !sheet_fast;
        if was_solid && !entry_closed && !self.drag_pre_oriented {
            orient_outward(mesh, DEFAULT_SEED, Some(&[face]));
        }
        let before: HashSet<FaceId> = mesh.face_ids().into_iter().collect();
        let base_attrs = mesh.face(face).attrs.clone();
        let base_pos = mesh.face_positions(face);
        let mut cap_face: Option<FaceId> = None;
        let mut far: Option<(Option<FaceId>, FaceAttrs)> = None;
        match through {
            Some(th) => {
                far = Some(through_ops(mesh, &base_pos, &base, th));
            }
            None => {
                cap_face = extrude_ops(
                    mesh,
                    &base_pos,
                    &base,
                    &top,
                    &base_holes,
                    &top_holes,
                    attached,
                );
            }
        }
        if !base_attrs.is_empty() {
            if let Some(cf) = cap_face.filter(|&f| mesh.has_face(f)) {
                mesh.face_mut(cf).attrs = base_attrs.clone();
            }
        }
        if let Some((Some(ff), attrs)) = far {
            if !attrs.is_empty() && mesh.has_face(ff) {
                mesh.face_mut(ff).attrs = attrs;
            }
        }
        let mut new_faces: HashSet<FaceId> = mesh
            .face_ids()
            .into_iter()
            .filter(|f| !before.contains(f))
            .collect();
        if self.keep_base && attached_any && d < 0.0 {
            for &f in &new_faces {
                let fnm = mesh.face_normal(f).unit_or_zero();
                if fnm.dot(normal).abs() > 0.999
                    && (mesh.face_centroid(f) - top[0]).dot(normal).abs() < 1e-4
                {
                    mesh.face_mut(f).interior = true;
                }
            }
        }
        let mut seed: Vec<DVec3> = base.iter().chain(top.iter()).copied().collect();
        for (hb, ht) in base_holes.iter().zip(top_holes.iter()) {
            seed.extend(hb.iter().copied());
            seed.extend(ht.iter().copied());
        }
        let seedkeys: HashSet<Key> = seed.iter().map(|&p| tkey(p)).collect();
        let solid = attached_any && was_solid;
        run_stitch(
            mesh,
            &seed,
            Some(&mut new_faces),
            !solid && !self.keep_base,
            !solid,
        );
        if solid {
            rebuild_planes_fixpoint(mesh, new_faces, &seed, self.keep_base, d < 0.0);
        }
        if !sheet_fast || is_closed(mesh) {
            let touched = touched_faces(mesh, &seedkeys);
            orient_outward(mesh, DEFAULT_SEED, touched.as_deref());
        }
    }
}

/// Every boundary position of `face` — outer loop and hole rims.
pub fn cap_loop_positions(mesh: &Mesh, face: FaceId) -> Vec<DVec3> {
    let mut pts = mesh.face_positions(face);
    for h in mesh.face_hole_positions(face) {
        pts.extend(h);
    }
    pts
}

/// The normal a drag signs `extrusion` along: the face's own, or — on an
/// OPEN shell — the surface's when the face disagrees with its coplanar
/// neighbours.
pub fn outward_normal(mesh: &Mesh, face: FaceId) -> DVec3 {
    if is_closed(mesh) || mesh_is_flat(mesh) {
        return mesh.face_normal(face);
    }
    surface_normal(mesh, face)
}

/// `face`'s normal, turned to agree with its coplanar neighbours.
pub fn surface_normal(mesh: &Mesh, face: FaceId) -> DVec3 {
    let n = mesh.face_normal(face).unit_or_zero();
    let on_loop: HashSet<VertexId> = mesh.face(face).outer.iter().copied().collect();
    let (mut agree, mut disagree) = (0, 0);
    for &v in &mesh.face(face).outer {
        for &e in &mesh.verts[v].edges {
            let ed = mesh.edge(e);
            if !on_loop.contains(&ed.v0) || !on_loop.contains(&ed.v1) {
                continue;
            }
            for &other in &ed.faces {
                if other == face {
                    continue;
                }
                let d = mesh.face_normal(other).unit_or_zero().dot(n);
                if d.abs() > 0.999 {
                    if d > 0.0 {
                        agree += 1;
                    } else {
                        disagree += 1;
                    }
                }
            }
        }
    }
    if disagree > agree {
        -n
    } else {
        n
    }
}

/// Counts + sorted vertex keys: enough to detect "the push dissolved itself
/// back to the original solid".
pub fn mesh_fingerprint(mesh: &Mesh) -> (usize, usize, Vec<Key>) {
    let mut keys: Vec<Key> = mesh.verts.iter().map(|(_, v)| tkey(v.position)).collect();
    keys.sort();
    (mesh.faces.len(), mesh.edges.len(), keys)
}

/// Hide the *new* edges between two faces meeting at a shallow dihedral.
pub fn soften_curve_facets(mesh: &mut Mesh, before_edges: &HashSet<xtrude_core::EdgeId>) {
    for e in mesh.edge_ids() {
        let ed = mesh.edge(e);
        if before_edges.contains(&e) || ed.soft || ed.faces.len() != 2 {
            continue;
        }
        let d = mesh
            .face_normal(ed.faces[0])
            .unit_or_zero()
            .dot(mesh.face_normal(ed.faces[1]).unit_or_zero());
        if CURVE_FACET_COS < d && d < 0.99995 {
            mesh.edge_mut(e).soft = true;
        }
    }
}

/// The faces carrying one of the operation's seed positions, or `None`.
pub fn touched_faces(mesh: &Mesh, seedkeys: &HashSet<Key>) -> Option<Vec<FaceId>> {
    let hit: Vec<FaceId> = mesh
        .face_ids()
        .into_iter()
        .filter(|&f| {
            mesh.face_positions(f)
                .iter()
                .any(|&p| seedkeys.contains(&tkey(p)))
        })
        .collect();
    (!hit.is_empty()).then_some(hit)
}

thread_local! {
    static PLANE_ORDER: std::cell::RefCell<Option<Vec<usize>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test hook: visit each fixpoint round's planes in this order (indices
/// into the sorted list; the rest follow in order), to prove the rebuild
/// order-independent.
#[doc(hidden)]
pub fn set_plane_order(order: Option<Vec<usize>>) {
    PLANE_ORDER.with(|o| *o.borrow_mut() = order);
}

fn permuted<T: Copy>(items: &[T], order: &[usize]) -> Vec<T> {
    let mut out: Vec<T> = order
        .iter()
        .filter_map(|&i| items.get(i).copied())
        .collect();
    out.extend(items.iter().skip(order.len()).copied());
    out
}

/// Deterministic root-fix (path C): recompute each touched plane's faces
/// from its edges, to a fixpoint.
pub fn rebuild_planes_fixpoint(
    mesh: &mut Mesh,
    mut fresh: HashSet<FaceId>,
    seed: &[DVec3],
    keep_mode: bool,
    removing: bool,
) {
    let mut op_rims: Vec<(DVec3, DVec3)> = Vec::new();
    let mut fresh_sorted: Vec<FaceId> = fresh.iter().copied().collect();
    fresh_sorted.sort();
    for &f in &fresh_sorted {
        if !mesh.has_face(f) {
            continue;
        }
        for lp in mesh.face(f).loops() {
            let c = lp.len();
            for i in 0..c {
                op_rims.push((mesh.pos(lp[i]), mesh.pos(lp[(i + 1) % c])));
            }
        }
    }
    let mut cache = RebuildCache::new();
    let opts = RebuildOpts {
        keep_mode,
        removing,
        prune: true,
    };
    for _ in 0..4 {
        let mut fresh_list: Vec<FaceId> = fresh.iter().copied().collect();
        fresh_list.sort();
        cache.seed_faces = fresh.clone();
        let mut planes: std::collections::BTreeMap<(i64, i64, i64, i64), (DVec3, DVec3)> =
            std::collections::BTreeMap::new();
        for (o, n) in seam_planes(mesh, &fresh_list) {
            planes.entry(plane_key(o, n).0).or_insert((o, n));
        }
        for (o, n) in crack_planes(mesh) {
            planes.entry(plane_key(o, n).0).or_insert((o, n));
        }
        let dropped = std::mem::take(&mut cache.dropped);
        for (o, n) in seam_planes_along(mesh, &dropped) {
            planes.entry(plane_key(o, n).0).or_insert((o, n));
        }
        let mut changed = false;
        let mut planes: Vec<(DVec3, DVec3)> = planes.into_values().collect();
        PLANE_ORDER.with(|o| {
            if let Some(order) = o.borrow().as_ref() {
                planes = permuted(&planes, order);
            }
        });
        for (origin, plane_n) in planes {
            let before: HashSet<FaceId> = mesh.face_ids().into_iter().collect();
            let fl: Vec<FaceId> = {
                let mut v: Vec<FaceId> = fresh.iter().copied().collect();
                v.sort();
                v
            };
            if apply_rebuild(
                mesh,
                origin,
                plane_n,
                &fl,
                opts,
                Some(&mut op_rims),
                Some(&mut cache),
            ) {
                changed = true;
                fresh.extend(mesh.face_ids().into_iter().filter(|f| !before.contains(f)));
            }
        }
        if !changed {
            break;
        }
        fresh.retain(|&f| mesh.has_face(f));
    }
    run_stitch(mesh, seed, None, false, true);
}

/// A through-hole target: `(far_face, back_loop, remainder)`.
type Through = (FaceId, Vec<DVec3>, Option<Vec<DVec3>>);

/// If pushing `face` by `d` reaches a parallel face on the far side of the
/// solid, the face to punch and the opening projected onto it.
pub fn find_through_face(mesh: &Mesh, face: FaceId, d: f64) -> Option<Through> {
    let fnm = mesh.face_normal(face).unit_or_zero();
    let push = if d > 0.0 { fnm } else { -fnm };
    let origin = mesh.face_centroid(face);
    let fpos = mesh.face_positions(face);
    let mut best: Option<(f64, Through)> = None;
    for g in mesh.face_ids() {
        if g == face {
            continue;
        }
        if fnm.dot(mesh.face_normal(g).unit_or_zero()).abs() < 0.999 {
            continue;
        }
        let dist = (mesh.face_centroid(g) - origin).dot(push);
        if dist <= 1e-4 || dist > d.abs() + 1e-4 {
            continue;
        }
        let back: Vec<DVec3> = fpos.iter().map(|&v| v + push * dist).collect();
        let gp = mesh.poly_face(g);
        if loop_inside_face(&gp, &back, false) {
            if best.as_ref().is_none_or(|b| dist < b.0) {
                best = Some((dist, (g, back, None)));
            }
            continue;
        }
        if (dist - d.abs()).abs() <= 1e-4 {
            if let Some(rem) = subtract_loop_from_face(&gp, &back) {
                if best.as_ref().is_none_or(|b| dist < b.0) {
                    best = Some((dist, (g, back, Some(rem))));
                }
            }
        }
    }
    best.map(|(_, through)| through)
}

/// The naive extrusion, before any rebuild; returns the cap face.
pub fn extrude_ops(
    mesh: &mut Mesh,
    base_pos: &[DVec3],
    base: &[DVec3],
    top: &[DVec3],
    base_holes: &[Vec<DVec3>],
    top_holes: &[Vec<DVec3>],
    attached: bool,
) -> Option<FaceId> {
    let count = base.len();
    if attached {
        delete_face_by_loop(mesh, base_pos);
    }
    for i in 0..count {
        let _ = add_edge_stamped(mesh, top[i], top[(i + 1) % count], None, None);
    }
    for i in 0..count {
        let _ = add_edge_stamped(mesh, base[i], top[i], None, None);
    }
    let cap = add_face_auto(mesh, top, top_holes, None, !attached);
    for (hb, ht) in base_holes.iter().zip(top_holes.iter()) {
        let hn = hb.len();
        for i in 0..hn {
            let _ = add_edge_stamped(mesh, ht[i], ht[(i + 1) % hn], None, None);
        }
        for i in 0..hn {
            let _ = add_edge_stamped(mesh, hb[i], ht[i], None, None);
        }
        for i in 0..hn {
            let j = (i + 1) % hn;
            add_face_auto(mesh, &[hb[i], hb[j], ht[j], ht[i]], &[], None, false);
        }
    }
    for i in 0..count {
        let j = (i + 1) % count;
        add_face_auto(mesh, &[base[i], base[j], top[j], top[i]], &[], None, false);
    }
    if attached {
        prune_orphans_at(mesh, base);
    }
    Some(cap)
}

/// A through-hole: punch the far face, join it to the front opening with a
/// tunnel, and sweep the dangling base edges. Returns the rebuilt far face
/// and the attrs it keeps.
fn through_ops(
    mesh: &mut Mesh,
    base_pos: &[DVec3],
    base: &[DVec3],
    through: Through,
) -> (Option<FaceId>, FaceAttrs) {
    let (far_face, back_loop, remainder) = through;
    let count = base.len();
    let far_pos = mesh.face_positions(far_face);
    let far_holes = mesh.face_hole_positions(far_face);
    let far_attrs = mesh.face(far_face).attrs.clone();
    delete_face_by_loop(mesh, base_pos);
    delete_face_by_loop(mesh, &far_pos);
    let far_new = match remainder {
        None => {
            let mut holes = far_holes.clone();
            holes.push(back_loop.clone());
            let f = add_face_auto(mesh, &far_pos, &holes, None, false);
            for i in 0..count {
                let _ =
                    add_edge_stamped(mesh, back_loop[i], back_loop[(i + 1) % count], None, None);
            }
            f
        }
        Some(rem) => add_face_auto(mesh, &rem, &far_holes, None, false),
    };
    for i in 0..count {
        let _ = add_edge_stamped(mesh, base[i], back_loop[i], None, None);
    }
    for i in 0..count {
        let j = (i + 1) % count;
        add_face_auto(
            mesh,
            &[base[i], base[j], back_loop[j], back_loop[i]],
            &[],
            None,
            false,
        );
    }
    let mut prune: Vec<DVec3> = base.to_vec();
    prune.extend(back_loop.iter().copied());
    prune_orphans_at(mesh, &prune);
    (Some(far_new), far_attrs)
}

/// The push of `face` of `target` by `dist` the way the fuzz bench does
/// (lock, set the extrusion, clamp), or `None` when the clamped distance
/// is too short to push.
fn plain_push(
    scene: &Scene,
    target: MeshId,
    face: FaceId,
    dist: f64,
    keep_base: bool,
) -> Option<PushPull> {
    let mut pp = PushPull::new();
    pp.setup_plain(scene.doc.mesh(target), target, face);
    pp.keep_base = keep_base;
    pp.compute_inward_limit(scene.doc.mesh(target));
    pp.extrusion = dist;
    pp.clamp_extrusion();
    (pp.extrusion.abs() >= 1e-6).then_some(pp)
}

/// Push `face` of `target` by `dist` as one undo step (see [`plain_push`]).
/// Returns whether a commit happened.
pub fn push_face(
    scene: &mut Scene,
    history: &mut History,
    target: MeshId,
    face: FaceId,
    dist: f64,
    keep_base: bool,
) -> bool {
    let Some(mut pp) = plain_push(scene, target, face, dist, keep_base) else {
        return false;
    };
    pp.commit(scene, history);
    true
}

/// [`push_face`] inside a command that is already running: the scene
/// changes, and the history records nothing of its own. Returns the push's
/// final state (its `outcome()`, `normal`, `extrusion` and the base's
/// `cap_positions`), or `None` when nothing was pushed (no such face, or a
/// distance too short).
pub fn push_face_in_scene(
    scene: &mut Scene,
    target: MeshId,
    face: FaceId,
    dist: f64,
    keep_base: bool,
) -> Result<Option<PushPull>, crate::history::CmdError> {
    if !scene.doc.mesh(target).has_face(face) {
        return Ok(None);
    }
    let Some(mut pp) = plain_push(scene, target, face, dist, keep_base) else {
        return Ok(None);
    };
    if pp.extrusion.abs() < MIN_EXTRUDE {
        return Ok(None);
    }
    pp.mutate_or_top_out(scene)?;
    Ok(Some(pp))
}
