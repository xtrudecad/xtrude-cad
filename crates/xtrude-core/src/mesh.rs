// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shared-vertex, non-manifold connectivity mesh.
//!
//! A [`Vertex`] is a first-class **shared** entity:
//!
//! - an [`Edge`] references two vertices and knows its incident faces (a
//!   *radial* list that may hold more than two — non-manifold is fine:
//!   three faces meet where two sides and a bottom join);
//! - a [`Face`] is bounded by loops of shared vertices, with optional holes;
//! - the [`Mesh`] welds coincident positions to a single vertex on insert
//!   ("sticky" geometry) and maintains the incidence as geometry is
//!   added/removed.
//!
//! This is the classic surface-modeller model. Moving a vertex moves every
//! edge and face that references it for free — no position-matching.
//!
//! Design notes: entities live in [`Arena`]s and are referred to by typed
//! ids. `Mesh: Clone` is the identity-preserving snapshot.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};

use indexmap::{IndexMap, IndexSet};

use crate::arena::{Arena, Id};
use crate::attrs::{Back, FaceAttrs};
use crate::math::{newell, DVec3};

/// Weld tolerance — two positions within this many decimals (~0.1 mm at
/// metric scale) are the same vertex. Same value as `topology`.
pub const KEY_DECIMALS: i32 = 4;
const KEY_SCALE: f64 = 1e4;

/// Distance tolerance for the stitch pass (a point this close to an edge
/// counts as on it). Matches the weld key resolution.
pub const STITCH_TOL: f64 = 1e-4;

/// Monotonic id for drawn curves (circle/arc). Segments of one curve share
/// it so selection groups them as one curve. Session-scoped; a document load
/// bumps it past any stored ids so new curves stay unique.
static CURVE_COUNTER: AtomicI64 = AtomicI64::new(1);

pub type VertexId = Id<Vertex>;
pub type EdgeId = Id<Edge>;
pub type FaceId = Id<Face>;

/// Integer weld cell of a position (same partition as rounding to
/// `KEY_DECIMALS`).
pub type Key = (i64, i64, i64);

#[inline]
pub fn key(p: DVec3) -> Key {
    (
        crate::math::round_even_int(p.x * KEY_SCALE),
        crate::math::round_even_int(p.y * KEY_SCALE),
        crate::math::round_even_int(p.z * KEY_SCALE),
    )
}

/// Keep new curves unique after loading stored ids.
pub fn bump_curve_counter_past(cid: i64) {
    CURVE_COUNTER.fetch_max(cid + 1, Ordering::SeqCst);
}

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum MeshError {
    #[error("degenerate edge: endpoints weld to one vertex")]
    DegenerateEdge,
    #[error("stale entity reference")]
    Stale,
    #[error("{0}")]
    Other(String),
}

/// A shared point in the mesh.
#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: DVec3,
    /// Incident edges, maintained by the mesh.
    pub edges: Vec<EdgeId>,
}

/// A segment between two shared vertices.
#[derive(Clone, Debug)]
pub struct Edge {
    pub v0: VertexId,
    pub v1: VertexId,
    /// The *radial* list of incident faces — 0, 1, 2, or more.
    pub faces: Vec<FaceId>,
    /// A curve segment (circle/arc): kept in the topology but hidden from
    /// the edge render, so the curve reads smooth.
    pub soft: bool,
    /// Fully invisible: unlike soft it draws neither as a line nor as a
    /// profile/silhouette.
    pub hidden: bool,
    /// Segments of one drawn curve share a `curve` id.
    pub curve: Option<i64>,
    /// Layer / tag name (`None` = default layer).
    pub layer: Option<String>,
}

impl Edge {
    fn new(v0: VertexId, v1: VertexId) -> Self {
        Edge {
            v0,
            v1,
            faces: Vec::new(),
            soft: false,
            hidden: false,
            curve: None,
            layer: None,
        }
    }

    /// The endpoint that is not `v`.
    #[inline]
    pub fn other(&self, v: VertexId) -> VertexId {
        if v == self.v0 {
            self.v1
        } else {
            self.v0
        }
    }

    #[inline]
    pub fn has(&self, v: VertexId) -> bool {
        v == self.v0 || v == self.v1
    }

    /// Everything an edge carries besides its two ends.
    pub fn flags(&self) -> EdgeFlags {
        EdgeFlags {
            soft: self.soft,
            curve: self.curve,
            layer: self.layer.clone(),
            hidden: self.hidden,
        }
    }

    pub fn stamp_flags(&mut self, f: &EdgeFlags) {
        self.soft = f.soft;
        self.curve = f.curve;
        self.layer = f.layer.clone();
        self.hidden = f.hidden;
    }

    /// True when there is nothing to carry: a bare, visible, untagged edge.
    pub fn is_plain(&self) -> bool {
        self.flags() == EdgeFlags::default()
    }
}

/// Everything an edge carries besides its two ends. Any code that rebuilds
/// a mesh from bare coordinates has to carry these across.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeFlags {
    pub soft: bool,
    pub curve: Option<i64>,
    pub layer: Option<String>,
    pub hidden: bool,
}

/// A planar polygon: an outer `loop` of shared vertices, plus optional
/// inner `holes`.
#[derive(Clone, Debug)]
pub struct Face {
    pub outer: Vec<VertexId>,
    pub holes: Vec<Vec<VertexId>>,
    /// Interior partition: a face *inside* a solid rather than on its
    /// boundary; volumetric queries skip it.
    pub interior: bool,
    pub attrs: FaceAttrs,
}

impl Face {
    /// All loops: the outer one first, then the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<VertexId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

/// Owns the shared vertices, edges and faces and keeps their incidence in
/// sync. The single place geometry is mutated; all welding happens here.
#[derive(Clone, Default)]
pub struct Mesh {
    registry: HashMap<Key, VertexId>,
    pub verts: Arena<Vertex>,
    pub edges: Arena<Edge>,
    pub faces: Arena<Face>,
    /// Set by every mutation primitive to a fresh, process-wide unique
    /// number: a cheap "has this mesh changed since I looked" for caches.
    /// Unique (not a per-mesh count) so a snapshot restored by undo keeps
    /// naming exactly the state it holds, and a later edit can never reuse
    /// the number of a state a cache has seen.
    pub mut_serial: u64,
}

impl std::fmt::Debug for Mesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Mesh({} verts, {} edges, {} faces)",
            self.verts.len(),
            self.edges.len(),
            self.faces.len()
        )
    }
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn touch(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.mut_serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    // ---- Accessors ---------------------------------------------------------
    #[inline]
    pub fn pos(&self, v: VertexId) -> DVec3 {
        self.verts[v].position
    }

    #[inline]
    pub fn edge(&self, e: EdgeId) -> &Edge {
        &self.edges[e]
    }

    #[inline]
    pub fn face(&self, f: FaceId) -> &Face {
        &self.faces[f]
    }

    #[inline]
    pub fn face_mut(&mut self, f: FaceId) -> &mut Face {
        self.touch();
        &mut self.faces[f]
    }

    #[inline]
    pub fn edge_mut(&mut self, e: EdgeId) -> &mut Edge {
        self.touch();
        &mut self.edges[e]
    }

    pub fn vertex_ids(&self) -> Vec<VertexId> {
        self.verts.ids()
    }

    pub fn edge_ids(&self) -> Vec<EdgeId> {
        self.edges.ids()
    }

    pub fn face_ids(&self) -> Vec<FaceId> {
        self.faces.ids()
    }

    pub fn has_face(&self, f: FaceId) -> bool {
        self.faces.contains(f)
    }

    pub fn has_edge(&self, e: EdgeId) -> bool {
        self.edges.contains(e)
    }

    pub fn has_vertex(&self, v: VertexId) -> bool {
        self.verts.contains(v)
    }

    /// Edge endpoints as positions (`edge.a`, `edge.b`).
    #[inline]
    pub fn edge_ab(&self, e: EdgeId) -> (DVec3, DVec3) {
        let ed = &self.edges[e];
        (self.pos(ed.v0), self.pos(ed.v1))
    }

    pub fn edge_length(&self, e: EdgeId) -> f64 {
        let (a, b) = self.edge_ab(e);
        (b - a).length()
    }

    /// `face.vertices`: the outer loop as positions.
    pub fn face_positions(&self, f: FaceId) -> Vec<DVec3> {
        self.faces[f].outer.iter().map(|&v| self.pos(v)).collect()
    }

    /// `face.holes`: the hole loops as positions.
    pub fn face_hole_positions(&self, f: FaceId) -> Vec<Vec<DVec3>> {
        self.faces[f]
            .holes
            .iter()
            .map(|h| h.iter().map(|&v| self.pos(v)).collect())
            .collect()
    }

    pub fn loop_positions(&self, lp: &[VertexId]) -> Vec<DVec3> {
        lp.iter().map(|&v| self.pos(v)).collect()
    }

    /// Faces touching this vertex (via its incident edges).
    pub fn vertex_faces(&self, v: VertexId) -> Vec<FaceId> {
        let mut out: IndexSet<FaceId> = IndexSet::new();
        for &e in &self.verts[v].edges {
            out.extend(self.edges[e].faces.iter().copied());
        }
        out.into_iter().collect()
    }

    // ---- Face geometry (over shared positions) ----------------------------
    /// Newell's area-weighted normal, in double precision.
    pub fn face_newell(&self, f: FaceId) -> DVec3 {
        let lp = &self.faces[f].outer;
        let n = lp.len();
        let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let c = self.pos(lp[i]);
            let nx_ = self.pos(lp[(i + 1) % n]);
            nx += (c.y - nx_.y) * (c.z + nx_.z);
            ny += (c.z - nx_.z) * (c.x + nx_.x);
            nz += (c.x - nx_.x) * (c.y + nx_.y);
        }
        DVec3::new(nx, ny, nz)
    }

    /// Unit face normal; `(0, 0, 1)` for a degenerate face. Divided by hand
    /// (never a zero "wildcard" normal, which slivers would match).
    pub fn face_normal(&self, f: FaceId) -> DVec3 {
        if self.faces[f].outer.len() < 3 {
            return DVec3::Z;
        }
        let n = self.face_newell(f);
        let len = n.length();
        if len < 1e-9 {
            return DVec3::Z;
        }
        n / len
    }

    pub fn face_area(&self, f: FaceId) -> f64 {
        if self.faces[f].outer.len() < 3 {
            return 0.0;
        }
        0.5 * self.face_newell(f).length()
    }

    pub fn face_centroid(&self, f: FaceId) -> DVec3 {
        let lp = &self.faces[f].outer;
        if lp.is_empty() {
            return DVec3::ZERO;
        }
        let mut s = DVec3::ZERO;
        for &v in lp {
            s += self.pos(v);
        }
        s / lp.len() as f64
    }

    pub fn face_triangulate(&self, f: FaceId, normal: Option<DVec3>) -> Vec<[DVec3; 3]> {
        if self.faces[f].outer.len() < 3 {
            return Vec::new();
        }
        let n = normal.unwrap_or_else(|| self.face_normal(f));
        crate::triangulate::triangulate(
            &self.face_positions(f),
            &self.face_hole_positions(f),
            Some(n),
        )
    }

    // ---- Vertices ----------------------------------------------------------
    fn probe_neighbours(&self, k: Key, position: DVec3) -> Option<VertexId> {
        let step = 1e-4;
        let tol2 = step * step;
        let (kx, ky, kz) = k;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    if let Some(&n) = self.registry.get(&(kx + dx, ky + dy, kz + dz)) {
                        if (self.pos(n) - position).length_squared() < tol2 {
                            return Some(n);
                        }
                    }
                }
            }
        }
        None
    }

    fn lookup(&self, position: DVec3) -> Option<VertexId> {
        let k = key(position);
        if let Some(&v) = self.registry.get(&k) {
            return Some(v);
        }
        self.probe_neighbours(k, position)
    }

    /// Get-or-create the shared vertex at `position` (welds coincident
    /// points to one vertex).
    pub fn vertex(&mut self, position: DVec3) -> VertexId {
        let k = key(position);
        if let Some(&v) = self.registry.get(&k) {
            return v;
        }
        if let Some(v) = self.probe_neighbours(k, position) {
            return v;
        }
        let v = self.verts.insert(Vertex {
            position,
            edges: Vec::new(),
        });
        self.registry.insert(k, v);
        v
    }

    /// The existing vertex at `position`, or `None`.
    pub fn vertex_at(&self, position: DVec3) -> Option<VertexId> {
        self.lookup(position)
    }

    // ---- Edges -------------------------------------------------------------
    /// The edge between two vertices (either orientation), or `None`.
    pub fn find_edge(&self, v0: VertexId, v1: VertexId) -> Option<EdgeId> {
        let (Some(a), Some(b)) = (self.verts.get(v0), self.verts.get(v1)) else {
            return None;
        };
        let (small, target, sv) = if a.edges.len() <= b.edges.len() {
            (a, v1, v0)
        } else {
            (b, v0, v1)
        };
        small
            .edges
            .iter()
            .copied()
            .find(|&e| self.edges[e].other(sv) == target)
    }

    /// A fresh curve id — for tools that re-create curve edges.
    pub fn next_curve_id() -> i64 {
        CURVE_COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    /// Mark every edge lying along the drawn `loop_points` path as one curve
    /// (a fresh id). Robust to splits: an edge is tagged when both its
    /// endpoints fall on one of the loop's segments.
    pub fn tag_curve(&mut self, loop_points: &[DVec3], closed: bool) -> Option<i64> {
        self.touch();
        if loop_points.len() < 2 {
            return None;
        }
        let cid = CURVE_COUNTER.load(Ordering::SeqCst);
        let mut pairs: Vec<(DVec3, DVec3)> = loop_points.windows(2).map(|w| (w[0], w[1])).collect();
        if closed {
            pairs.push((loop_points[loop_points.len() - 1], loop_points[0]));
        }
        let mut tagged = false;
        let ids = self.edges.ids();
        for e in ids {
            if self.edges[e].curve.is_some() {
                continue;
            }
            let (a, b) = self.edge_ab(e);
            for &(pa, pb) in &pairs {
                if crate::topology::point_on_seg_incl(a, pa, pb, crate::topology::SPLIT_TOLERANCE)
                    && crate::topology::point_on_seg_incl(
                        b,
                        pa,
                        pb,
                        crate::topology::SPLIT_TOLERANCE,
                    )
                {
                    self.edges[e].curve = Some(cid);
                    tagged = true;
                    break;
                }
            }
        }
        if tagged {
            CURVE_COUNTER.fetch_max(cid + 1, Ordering::SeqCst);
            Some(cid)
        } else {
            None
        }
    }

    /// All edges sharing `edge`'s curve id (`[edge]` if it has none).
    pub fn curve_edges(&self, edge: EdgeId) -> Vec<EdgeId> {
        match self.edges[edge].curve {
            None => vec![edge],
            Some(cid) => self
                .edges
                .iter()
                .filter(|(_, e)| e.curve == Some(cid))
                .map(|(id, _)| id)
                .collect(),
        }
    }

    /// Re-partition curve ids into contiguous contours. A curve breaks at any
    /// vertex where other geometry attaches or where its own connectivity is
    /// not a simple chain. A pristine circle stays one curve.
    pub fn resplit_curves(&mut self) {
        self.touch();
        let mut by_id: IndexMap<i64, Vec<EdgeId>> = IndexMap::new();
        for (id, e) in self.edges.iter() {
            if let Some(c) = e.curve {
                by_id.entry(c).or_default().push(id);
            }
        }
        for (cid, edges) in by_id {
            let eset: HashSet<EdgeId> = edges.iter().copied().collect();
            let is_break = |m: &Mesh, v: VertexId| -> bool {
                let ve = &m.verts[v].edges;
                let cdeg = ve.iter().filter(|e| eset.contains(e)).count();
                cdeg != 2 || ve.len() != cdeg
            };
            let mut unvisited: IndexSet<EdgeId> = edges.iter().copied().collect();
            let mut chains: Vec<Vec<EdgeId>> = Vec::new();
            let seeds: Vec<EdgeId> = edges
                .iter()
                .copied()
                .filter(|&e| is_break(self, self.edges[e].v0) || is_break(self, self.edges[e].v1))
                .collect();
            for seed in seeds.iter().chain(edges.iter()).copied() {
                if !unvisited.contains(&seed) {
                    continue;
                }
                let sd = &self.edges[seed];
                let start_v = if is_break(self, sd.v0) {
                    sd.v0
                } else if is_break(self, sd.v1) {
                    sd.v1
                } else {
                    sd.v0
                };
                let mut chain = Vec::new();
                let mut v = start_v;
                let mut e = seed;
                while unvisited.contains(&e) {
                    unvisited.shift_remove(&e);
                    chain.push(e);
                    v = self.edges[e].other(v);
                    if is_break(self, v) {
                        break;
                    }
                    let nxt = self.verts[v]
                        .edges
                        .iter()
                        .copied()
                        .find(|n| unvisited.contains(n));
                    match nxt {
                        Some(n) => e = n,
                        None => break,
                    }
                }
                chains.push(chain);
            }
            if chains.len() <= 1 {
                continue; // still one contour — keep the id stable
            }
            for (i, chain) in chains.iter().enumerate() {
                let new_id = if i == 0 {
                    cid
                } else {
                    CURVE_COUNTER.fetch_add(1, Ordering::SeqCst)
                };
                for &e in chain {
                    self.edges[e].curve = Some(new_id);
                }
            }
        }
    }

    fn link_edge(&mut self, v0: VertexId, v1: VertexId) -> EdgeId {
        self.touch();
        if let Some(e) = self.find_edge(v0, v1) {
            return e;
        }
        let e = self.edges.insert(Edge::new(v0, v1));
        push_unique(&mut self.verts[v0].edges, e);
        push_unique(&mut self.verts[v1].edges, e);
        e
    }

    /// Add a free edge between two positions (welding endpoints, deduping
    /// the edge). Returns the existing edge if it is already there.
    pub fn add_edge(&mut self, a: DVec3, b: DVec3) -> Result<EdgeId, MeshError> {
        let v0 = self.vertex(a);
        let v1 = self.vertex(b);
        if v0 == v1 {
            return Err(MeshError::DegenerateEdge);
        }
        Ok(self.link_edge(v0, v1))
    }

    /// Detach and drop an edge. Callers remove any incident faces first
    /// when needed; this does not cascade.
    pub fn remove_edge(&mut self, e: EdgeId) {
        self.touch();
        let Some(ed) = self.edges.get(e) else { return };
        let (v0, v1) = (ed.v0, ed.v1);
        if let Some(v) = self.verts.get_mut(v0) {
            v.edges.retain(|&x| x != e);
        }
        if let Some(v) = self.verts.get_mut(v1) {
            v.edges.retain(|&x| x != e);
        }
        self.edges.remove(e);
    }

    /// Drop vertices nothing references any more. Returns how many went.
    pub fn prune_orphan_vertices(&mut self) -> usize {
        let mut used: HashSet<VertexId> = HashSet::new();
        for (_, f) in self.faces.iter() {
            for lp in f.loops() {
                used.extend(lp.iter().copied());
            }
        }
        let doomed: Vec<VertexId> = self
            .verts
            .iter()
            .filter(|(id, v)| v.edges.is_empty() && !used.contains(id))
            .map(|(id, _)| id)
            .collect();
        if doomed.is_empty() {
            return 0;
        }
        let dset: HashSet<VertexId> = doomed.iter().copied().collect();
        for &v in &doomed {
            self.verts.remove(v);
        }
        // The registry is cleaned by IDENTITY: a moved vertex no longer
        // hashes to the key it was filed under.
        self.registry.retain(|_, v| !dset.contains(v));
        self.touch();
        doomed.len()
    }

    // ---- Bulk construction (imports / document load) --------------------------
    /// Weld positions into shared vertices, in order — the exact equivalent
    /// of calling [`Mesh::vertex`] on each point. Returns the distinct
    /// vertices touched and, per input point, its index into that list.
    pub fn bulk_weld(&mut self, pos: &[DVec3]) -> (Vec<VertexId>, Vec<usize>) {
        let mut vobjs: Vec<VertexId> = Vec::new();
        let mut vindex: HashMap<VertexId, usize> = HashMap::new();
        let mut ids = Vec::with_capacity(pos.len());
        for &p in pos {
            let v = self.vertex(p);
            let i = *vindex.entry(v).or_insert_with(|| {
                vobjs.push(v);
                vobjs.len() - 1
            });
            ids.push(i);
        }
        (vobjs, ids)
    }

    /// Create the edges `(vobjs[ia[i]], vobjs[ib[i]])`: endpoints that welded
    /// to one vertex are skipped, duplicates return the existing edge.
    pub fn add_edges_welded(
        &mut self,
        vobjs: &[VertexId],
        ia: &[usize],
        ib: &[usize],
        flags: Option<&[EdgeFlags]>,
    ) -> Vec<Option<EdgeId>> {
        self.touch();
        let mut out = Vec::with_capacity(ia.len());
        for i in 0..ia.len() {
            let v0 = vobjs[ia[i]];
            let v1 = vobjs[ib[i]];
            if v0 == v1 {
                out.push(None);
                continue; // degenerate: endpoints welded
            }
            let e = self.link_edge(v0, v1);
            if let Some(flags) = flags {
                let f = &flags[i];
                let ed = &mut self.edges[e];
                if f.soft {
                    ed.soft = true;
                }
                if f.hidden {
                    ed.hidden = true;
                }
                if f.curve.is_some() {
                    ed.curve = f.curve;
                }
                if f.layer.is_some() {
                    ed.layer = f.layer.clone();
                }
            }
            out.push(Some(e));
        }
        out
    }

    /// Create a face from pre-welded vertex loops — `add_face` minus the
    /// nested-hole filter (callers run `topology::maximal_holes` while the
    /// loops are still positions).
    pub fn add_face_welded(&mut self, outer: Vec<VertexId>, holes: Vec<Vec<VertexId>>) -> FaceId {
        self.touch();
        let face = self.faces.insert(Face {
            outer: outer.clone(),
            holes: holes.clone(),
            interior: false,
            attrs: FaceAttrs::default(),
        });
        for lp in std::iter::once(&outer).chain(holes.iter()) {
            let n = lp.len();
            for i in 0..n {
                let e = self.link_edge(lp[i], lp[(i + 1) % n]);
                self.edges[e].faces.push(face);
            }
        }
        face
    }

    /// Create faces from pre-welded corners — the bulk equivalent of
    /// [`Mesh::add_face`] per face (minus the nested-hole filter), in bulk
    /// loading order: every boundary edge first (first
    /// occurrence's orientation), then the faces, each registered on its
    /// edges in segment order. `ids` indexes every ring corner into
    /// `vobjs`, rings concatenated in face order (outer first, then that
    /// face's holes); `ring_sizes` is each ring's corner count and
    /// `ring_counts` how many rings each face owns.
    pub fn add_faces_welded(
        &mut self,
        vobjs: &[VertexId],
        ids: &[usize],
        ring_sizes: &[usize],
        ring_counts: &[usize],
        attrs: &[Option<FaceAttrs>],
    ) -> Vec<FaceId> {
        self.touch();
        let mut starts = Vec::with_capacity(ring_sizes.len());
        let mut k = 0;
        for &n in ring_sizes {
            starts.push(k);
            k += n;
        }
        for (r, &n) in ring_sizes.iter().enumerate() {
            let s0 = starts[r];
            for i in 0..n {
                let a = vobjs[ids[s0 + i]];
                let b = vobjs[ids[s0 + (i + 1) % n]];
                self.link_edge(a, b);
            }
        }
        let mut out = Vec::with_capacity(ring_counts.len());
        let mut r = 0;
        for (fi, &nr) in ring_counts.iter().enumerate() {
            let ring = |r: usize| -> Vec<VertexId> {
                (0..ring_sizes[r])
                    .map(|i| vobjs[ids[starts[r] + i]])
                    .collect()
            };
            let outer = ring(r);
            let holes: Vec<Vec<VertexId>> = (1..nr).map(|h| ring(r + h)).collect();
            r += nr;
            let face = self.faces.insert(Face {
                outer: outer.clone(),
                holes: holes.clone(),
                interior: false,
                attrs: attrs.get(fi).cloned().flatten().unwrap_or_default(),
            });
            for lp in std::iter::once(&outer).chain(holes.iter()) {
                let n = lp.len();
                for i in 0..n {
                    let e = self.link_edge(lp[i], lp[(i + 1) % n]);
                    self.edges[e].faces.push(face);
                }
            }
            out.push(face);
        }
        out
    }

    // ---- Faces -------------------------------------------------------------
    /// Add a face from position loops. Vertices weld, the boundary edges are
    /// created if missing (and the face is registered on each as an incident
    /// face — radial, so an edge can carry several).
    ///
    /// Nested holes are dropped here, at the single choke point every face
    /// creation path goes through: a hole inside another hole of the same
    /// face corrupts earcut and lets `is_closed` report a broken solid as
    /// watertight.
    pub fn add_face(&mut self, loop_positions: &[DVec3], hole_loops: &[Vec<DVec3>]) -> FaceId {
        let outer: Vec<VertexId> = loop_positions.iter().map(|&p| self.vertex(p)).collect();
        let raw_holes: Vec<Vec<DVec3>> = if hole_loops.len() > 1 {
            crate::topology::maximal_holes(hole_loops)
        } else {
            hole_loops.to_vec()
        };
        let holes: Vec<Vec<VertexId>> = raw_holes
            .iter()
            .map(|h| h.iter().map(|&p| self.vertex(p)).collect())
            .collect();
        self.add_face_welded(outer, holes)
    }

    /// The faces forming the same **curved surface** as `face` — the
    /// connected component reached by crossing **soft** edges.
    pub fn surface_of(&self, face: FaceId) -> Vec<FaceId> {
        let mut seen: IndexSet<FaceId> = IndexSet::new();
        seen.insert(face);
        let mut stack = vec![face];
        while let Some(f) = stack.pop() {
            for lp in self.faces[f].loops() {
                let n = lp.len();
                for i in 0..n {
                    let Some(e) = self.find_edge(lp[i], lp[(i + 1) % n]) else {
                        continue;
                    };
                    if !self.edges[e].soft {
                        continue;
                    }
                    for &g in &self.edges[e].faces {
                        if g != f && seen.insert(g) {
                            stack.push(g);
                        }
                    }
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Drop a face and detach it from its boundary edges' radial lists. The
    /// edges themselves stay.
    pub fn remove_face(&mut self, face: FaceId) {
        self.touch();
        let Some(f) = self.faces.get(face) else {
            return;
        };
        let loops: Vec<Vec<VertexId>> = f.loops().cloned().collect();
        for lp in &loops {
            let n = lp.len();
            for i in 0..n {
                if let Some(e) = self.find_edge(lp[i], lp[(i + 1) % n]) {
                    remove_first(&mut self.edges[e].faces, face);
                }
            }
        }
        self.faces.remove(face);
    }

    // ---- Re-link (undo support) -------------------------------------------
    /// Re-attach a previously removed edge (undo), preserving its id.
    pub fn relink_edge(&mut self, id: EdgeId, edge: Edge) {
        if self.edges.contains(id) {
            return;
        }
        let (v0, v1) = (edge.v0, edge.v1);
        self.edges.restore(id, edge);
        push_unique(&mut self.verts[v0].edges, id);
        push_unique(&mut self.verts[v1].edges, id);
        let (p0, p1) = (self.pos(v0), self.pos(v1));
        self.registry.entry(key(p0)).or_insert(v0);
        self.registry.entry(key(p1)).or_insert(v1);
        self.touch();
    }

    /// Re-attach a previously removed face (undo), recreating any missing
    /// boundary edges and re-registering incidence.
    pub fn relink_face(&mut self, id: FaceId, face: Face) {
        if self.faces.contains(id) {
            return;
        }
        let loops: Vec<Vec<VertexId>> = face.loops().cloned().collect();
        self.faces.restore(id, face);
        for lp in &loops {
            let n = lp.len();
            for i in 0..n {
                let e = self.link_edge(lp[i], lp[(i + 1) % n]);
                if !self.edges[e].faces.contains(&id) {
                    self.edges[e].faces.push(id);
                }
            }
        }
        self.touch();
    }

    // ---- Holes -------------------------------------------------------------
    /// Punch a hole into `face`. Welds the loop's vertices, ensures its
    /// boundary edges, and registers the face on them.
    pub fn add_hole(&mut self, face: FaceId, loop_positions: &[DVec3]) -> Vec<VertexId> {
        let lp: Vec<VertexId> = loop_positions.iter().map(|&p| self.vertex(p)).collect();
        self.faces[face].holes.push(lp.clone());
        let n = lp.len();
        for i in 0..n {
            let e = self.link_edge(lp[i], lp[(i + 1) % n]);
            if !self.edges[e].faces.contains(&face) {
                self.edges[e].faces.push(face);
            }
        }
        lp
    }

    // ---- Reset ---------------------------------------------------------------
    pub fn clear(&mut self) {
        self.touch();
        self.registry.clear();
        self.verts.clear();
        self.edges.clear();
        self.faces.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty() && self.edges.is_empty() && self.faces.is_empty()
    }

    // ---- Mutation ------------------------------------------------------------
    fn rekey(&mut self, v: VertexId, old: Key) {
        let new = key(self.verts[v].position);
        if old == new {
            return;
        }
        if self.registry.get(&old) == Some(&v) {
            self.registry.remove(&old);
        }
        self.registry.entry(new).or_insert(v);
    }

    /// Translate a vertex. Every edge and face referencing it follows. The
    /// registry is re-keyed so later welds find it at the new spot; merging
    /// with a vertex it lands on is a separate operation.
    pub fn move_vertex(&mut self, v: VertexId, delta: DVec3) {
        self.touch();
        let old = key(self.verts[v].position);
        self.verts[v].position += delta;
        self.rekey(v, old);
    }

    /// Put a vertex AT `position` — `move_vertex` with an absolute target.
    pub fn place_vertex(&mut self, v: VertexId, position: DVec3) {
        self.touch();
        let old = key(self.verts[v].position);
        self.verts[v].position = position;
        self.rekey(v, old);
    }

    /// Split `edge` at `position`, inserting a shared vertex; returns the
    /// two sub-edges. Every incident face gains the new vertex in its loop.
    pub fn split_edge(&mut self, edge: EdgeId, position: DVec3) -> (EdgeId, EdgeId) {
        let (v0, v1) = (self.edges[edge].v0, self.edges[edge].v1);
        let mid = self.vertex(position);
        if mid == v0 || mid == v1 {
            return (edge, edge);
        }
        let incident = self.edges[edge].faces.clone();
        self.remove_edge(edge);
        let e0 = self.link_edge(v0, mid);
        let e1 = self.link_edge(mid, v1);
        for face in incident {
            self.insert_into_face_loops(face, v0, v1, mid);
            if !self.edges[e0].faces.contains(&face) {
                self.edges[e0].faces.push(face);
            }
            if !self.edges[e1].faces.contains(&face) {
                self.edges[e1].faces.push(face);
            }
        }
        (e0, e1)
    }

    /// Insert `mid` between the consecutive `v0`/`v1` pair in the face's
    /// outer loop or one of its holes (whichever carries that edge).
    fn insert_into_face_loops(&mut self, face: FaceId, v0: VertexId, v1: VertexId, mid: VertexId) {
        let f = &mut self.faces[face];
        for lp in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
            let n = lp.len();
            for i in 0..n {
                let j = (i + 1) % n;
                if (lp[i] == v0 && lp[j] == v1) || (lp[i] == v1 && lp[j] == v0) {
                    lp.insert(i + 1, mid);
                    return;
                }
            }
        }
    }

    // ---- Stitch primitives -----------------------------------------------------
    /// Merge vertices occupying the same position. Edges and face loops are
    /// re-pointed onto the kept vertex; zero-length edges vanish, duplicate
    /// edges merge their incidence, and faces degenerated below three
    /// distinct vertices are dropped. Returns whether anything merged.
    pub fn weld_coincident(&mut self) -> bool {
        self.touch();
        let mut groups: IndexMap<Key, Vec<VertexId>> = IndexMap::new();
        for (id, v) in self.verts.iter() {
            groups.entry(key(v.position)).or_default().push(id);
        }
        let mut changed = false;
        for (_, vs) in groups {
            if vs.len() < 2 {
                continue;
            }
            let keep = vs[0];
            for &dup in &vs[1..] {
                self.weld_into(keep, dup);
            }
            changed = true;
        }
        // Cross-cell pass: two coincident points can straddle a rounding
        // boundary into different cells. Weld into the lexicographically
        // smaller key — a deterministic survivor.
        let tol2 = 1e-8;
        let mut merged = true;
        while merged {
            merged = false;
            'outer: for v in self.verts.ids() {
                if !self.verts.contains(v) {
                    continue;
                }
                let k = key(self.pos(v));
                if self.registry.get(&k) != Some(&v) {
                    continue;
                }
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let nk = (k.0 + dx, k.1 + dy, k.2 + dz);
                            if nk == k {
                                continue;
                            }
                            let Some(&w) = self.registry.get(&nk) else {
                                continue;
                            };
                            if w == v
                                || !self.verts.contains(w)
                                || (self.pos(w) - self.pos(v)).length_squared() >= tol2
                            {
                                continue;
                            }
                            let (kp, dp) = if k < nk { (v, w) } else { (w, v) };
                            self.weld_into(kp, dp);
                            changed = true;
                            merged = true;
                            break 'outer;
                        }
                    }
                }
            }
        }
        changed
    }

    /// Drop faces occupying the same *outer* edge cycle as another. When the
    /// pair differs in holes, the one with more holes survives. Returns how
    /// many were removed.
    pub fn dedupe_faces(&mut self) -> usize {
        self.touch();
        let mut seen: HashMap<Vec<(VertexId, VertexId)>, FaceId> = HashMap::new();
        let mut removed = 0;
        for f in self.faces.ids() {
            let sig = cycle_signature(&self.faces[f].outer);
            let Some(&other) = seen.get(&sig) else {
                seen.insert(sig, f);
                continue;
            };
            let (drop, keep) = if self.faces[f].holes.len() > self.faces[other].holes.len() {
                (other, f)
            } else {
                (f, other)
            };
            if !self.faces[drop].attrs.is_empty() && self.faces[keep].attrs.is_empty() {
                self.faces[keep].attrs = self.faces[drop].attrs.clone();
            }
            self.remove_face(drop);
            seen.insert(sig, keep);
            removed += 1;
        }
        removed
    }

    fn weld_into(&mut self, keep: VertexId, dup: VertexId) {
        let mut affected: IndexSet<FaceId> = IndexSet::new();
        for &e in &self.verts[dup].edges {
            affected.extend(self.edges[e].faces.iter().copied());
        }
        for e in self.verts[dup].edges.clone() {
            let other = self.edges[e].other(dup);
            let incident = self.edges[e].faces.clone();
            self.remove_edge(e);
            if other == keep {
                continue; // zero-length edge vanishes
            }
            let kept = self.link_edge(keep, other);
            for f in incident {
                if !self.edges[kept].faces.contains(&f) {
                    self.edges[kept].faces.push(f);
                }
            }
        }
        for f in affected {
            if !self.faces.contains(f) {
                continue;
            }
            {
                let fc = &mut self.faces[f];
                for lp in std::iter::once(&mut fc.outer).chain(fc.holes.iter_mut()) {
                    for x in lp.iter_mut() {
                        if *x == dup {
                            *x = keep;
                        }
                    }
                    let mut i = 0;
                    while lp.len() > 1 && i < lp.len() {
                        if lp[i] == lp[(i + 1) % lp.len()] {
                            let idx = if i + 1 < lp.len() {
                                i
                            } else {
                                i + 1 - lp.len()
                            };
                            lp.remove(idx);
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            let distinct: HashSet<VertexId> = self.faces[f].outer.iter().copied().collect();
            if distinct.len() < 3 {
                self.remove_face(f); // collapsed to a sliver
            }
        }
        let dpos = self.verts.get(dup).map(|v| v.position);
        self.verts.remove(dup);
        if let Some(p) = dpos {
            let k = key(p);
            if self.registry.get(&k) == Some(&dup) {
                self.registry.insert(k, keep);
            }
        }
        self.touch();
    }

    /// Split `edge` at the *existing* vertex `mid` on its interior. A
    /// sub-edge may already exist (the T-junction's other face owns it), in
    /// which case it is reused. Returns a reversible record.
    pub fn split_edge_at(&mut self, edge: EdgeId, mid: VertexId) -> StitchRecord {
        let old = self.edges[edge].clone();
        let (v0, v1) = (old.v0, old.v1);
        let incident = old.faces.clone();
        self.remove_edge(edge);
        let e0_new = self.find_edge(v0, mid).is_none();
        let e0 = self.link_edge(v0, mid);
        let e1_new = self.find_edge(mid, v1).is_none();
        let e1 = self.link_edge(mid, v1);
        // Sub-edges continue the split edge's curve/soft/layer identity.
        for (sub, was_new) in [(e0, e0_new), (e1, e1_new)] {
            if was_new {
                let s = &mut self.edges[sub];
                s.soft = old.soft;
                s.layer = old.layer.clone();
                s.curve = old.curve;
            }
        }
        for &face in &incident {
            self.insert_into_face_loops(face, v0, v1, mid);
            if !self.edges[e0].faces.contains(&face) {
                self.edges[e0].faces.push(face);
            }
            if !self.edges[e1].faces.contains(&face) {
                self.edges[e1].faces.push(face);
            }
        }
        StitchRecord::Split {
            edge: (edge, old),
            e0,
            e1,
            e0_new,
            e1_new,
            mid,
            faces: incident,
        }
    }

    /// Whether `v` is a redundant valence-2 collinear vertex whose two edges
    /// border the same faces.
    pub fn collapsible_vertex(&self, v: VertexId) -> bool {
        let ve = &self.verts[v].edges;
        if ve.len() != 2 {
            return false;
        }
        let (e1, e2) = (ve[0], ve[1]);
        let p = self.pos(v);
        let d1 = p - self.pos(self.edges[e1].other(v));
        let d2 = self.pos(self.edges[e2].other(v)) - p;
        if d1.length() < STITCH_TOL || d2.length() < STITCH_TOL {
            return false;
        }
        if d1.normalize().dot(d2.normalize()) < 0.999 {
            return false;
        }
        let s1: HashSet<FaceId> = self.edges[e1].faces.iter().copied().collect();
        let s2: HashSet<FaceId> = self.edges[e2].faces.iter().copied().collect();
        s1 == s2
    }

    /// Remove the redundant valence-2 collinear vertex `v`, merging its two
    /// edges into one. Returns a reversible record.
    pub fn collapse_vertex(&mut self, v: VertexId) -> StitchRecord {
        let ve = self.verts[v].edges.clone();
        let (e1, e2) = (ve[0], ve[1]);
        let n1 = self.edges[e1].other(v);
        let n2 = self.edges[e2].other(v);
        let faces: Vec<FaceId> = {
            let mut s: IndexSet<FaceId> = IndexSet::new();
            s.extend(self.edges[e1].faces.iter().copied());
            s.into_iter().collect()
        };
        let e1_rec = self.edges[e1].clone();
        let e2_rec = self.edges[e2].clone();
        self.remove_edge(e1);
        self.remove_edge(e2);
        let new_edge_existed = self.find_edge(n1, n2).is_some();
        let new_edge = self.link_edge(n1, n2);
        for &face in &faces {
            let fc = &mut self.faces[face];
            for lp in std::iter::once(&mut fc.outer).chain(fc.holes.iter_mut()) {
                if let Some(i) = lp.iter().position(|&x| x == v) {
                    lp.remove(i);
                }
            }
            if !self.edges[new_edge].faces.contains(&face) {
                self.edges[new_edge].faces.push(face);
            }
        }
        let vrec = self.verts[v].clone();
        self.verts.remove(v);
        let k = key(vrec.position);
        if self.registry.get(&k) == Some(&v) {
            self.registry.remove(&k);
        }
        self.touch();
        StitchRecord::Collapse {
            v: (v, vrec),
            e1: (e1, e1_rec),
            e2: (e2, e2_rec),
            n1,
            n2,
            new_edge,
            new_edge_new: !new_edge_existed,
            faces,
        }
    }

    /// Merge a set of mutually-coplanar, edge-connected faces into one face
    /// spanning the boundary of their union, removing every interior edge.
    pub fn dissolve_coplanar_region(&mut self, faces: &[FaceId]) -> Option<FaceId> {
        if faces.len() < 2 {
            return None;
        }
        let n0 = self.face_normal(faces[0]);
        // Coplanar regardless of winding sign.
        if faces[1..]
            .iter()
            .any(|&f| n0.dot(self.face_normal(f)).abs() < 0.999)
        {
            return None;
        }
        let mut counts: IndexMap<(VertexId, VertexId), usize> = IndexMap::new();
        for &f in faces {
            for lp in self.faces[f].loops() {
                let n = lp.len();
                for i in 0..n {
                    *counts.entry(ukey(lp[i], lp[(i + 1) % n])).or_insert(0) += 1;
                }
            }
        }
        if !counts.values().any(|&c| c >= 2) {
            return None; // not edge-connected
        }
        let mut outline = Vec::new();
        for &f in faces {
            for lp in self.faces[f].loops() {
                let n = lp.len();
                for i in 0..n {
                    let (u, w) = (lp[i], lp[(i + 1) % n]);
                    if counts[&ukey(u, w)] == 1 {
                        outline.push((u, w));
                    }
                }
            }
        }
        let mut loops_out = trace_loops(&outline)?;
        if loops_out.is_empty() {
            // The union has no boundary of its own: identical cycles stacked
            // on one another. Keep one face, drop the duplicates.
            if self.faces[faces[0]].attrs.is_empty() {
                self.faces[faces[0]].attrs = self.dominant_attrs(&faces[1..]);
            }
            for &f in &faces[1..] {
                self.remove_face(f);
            }
            return Some(faces[0]);
        }
        loops_out.sort_by(|a, b| {
            self.loop_area(b)
                .partial_cmp(&self.loop_area(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let outer = loops_out[0].clone();
        let holes: Vec<Vec<VertexId>> = loops_out[1..].to_vec();
        let interior: Vec<EdgeId> = counts
            .iter()
            .filter(|(_, &c)| c >= 2)
            .filter_map(|(&(a, b), _)| self.find_edge(a, b))
            .collect();
        let attrs = self.dominant_attrs(faces);
        for &f in faces {
            self.remove_face(f);
        }
        for e in interior {
            if self.edges.get(e).is_some_and(|ed| ed.faces.is_empty()) {
                self.remove_edge(e);
            }
        }
        let op = self.loop_positions(&outer);
        let hp: Vec<Vec<DVec3>> = holes.iter().map(|h| self.loop_positions(h)).collect();
        let merged = self.add_face(&op, &hp);
        self.faces[merged].attrs = attrs;
        Some(merged)
    }

    /// Dissolve a *redundant* coplanar edge, merging its two faces into one.
    /// Returns the new merged face, or `None` if the edge is not mergeable.
    pub fn dissolve_edge(&mut self, edge: EdgeId) -> Option<FaceId> {
        let ed = self.edges.get(edge)?;
        if ed.faces.len() != 2 {
            return None;
        }
        let (fa, fb) = (ed.faces[0], ed.faces[1]);
        if fa == fb {
            return None; // both sides are the same face (a slit)
        }
        let dot = self.face_normal(fa).dot(self.face_normal(fb));
        if dot.abs() < 0.999 {
            return None; // not coplanar
        }
        // Opposite-wound coplanar fragments are still one face — flip B.
        let loop_b: Vec<VertexId> = if dot > 0.0 {
            self.faces[fb].outer.clone()
        } else {
            self.faces[fb].outer.iter().rev().copied().collect()
        };
        let (v0, v1) = (ed.v0, ed.v1);
        let merged = splice_loops(&self.faces[fa].outer, &loop_b, v0, v1)?;
        let loop_positions = self.loop_positions(&merged);
        let hole_positions: Vec<Vec<DVec3>> = self.faces[fa]
            .holes
            .iter()
            .chain(self.faces[fb].holes.iter())
            .map(|h| self.loop_positions(h))
            .collect();
        let attrs = self.dominant_attrs(&[fa, fb]);
        self.remove_face(fa);
        self.remove_face(fb);
        self.remove_edge(edge);
        let mf = self.add_face(&loop_positions, &hole_positions);
        self.faces[mf].attrs = attrs;
        Some(mf)
    }

    /// The attrs of the largest-area face that carries any.
    pub fn dominant_attrs(&self, faces: &[FaceId]) -> FaceAttrs {
        let mut best = FaceAttrs::default();
        let mut best_area = -1.0;
        for &f in faces {
            let Some(fc) = self.faces.get(f) else {
                continue;
            };
            if !fc.attrs.is_empty() {
                let a = self.face_area(f);
                if a > best_area {
                    best = fc.attrs.clone();
                    best_area = a;
                }
            }
        }
        best
    }

    /// Newell-magnitude area of a vertex loop (orientation-independent).
    pub fn loop_area(&self, lp: &[VertexId]) -> f64 {
        0.5 * newell(&self.loop_positions(lp)).length()
    }

    /// Reverse `face`'s winding for the automatic orientation passes — which
    /// must not move paint from one side of a wall to the other. When the
    /// BACK has paint of its own, the two paints trade places; paint on one
    /// side follows the front.
    pub fn turn_face_over(&mut self, face: FaceId) {
        self.touch();
        let f = &mut self.faces[face];
        if let Some(Back::Own(back)) = f.attrs.back.clone() {
            let front = std::mem::take(&mut f.attrs.paint);
            f.attrs.back = None;
            f.attrs.paint = back;
            if !front.is_empty() {
                f.attrs.back = Some(Back::Own(front));
            }
        }
        f.outer.reverse();
        for h in f.holes.iter_mut() {
            h.reverse();
        }
    }

    /// Axis-aligned bounds of the vertices referenced by edges or faces.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        let mut seen = false;
        for (_, v) in self.verts.iter() {
            lo = lo.min(v.position);
            hi = hi.max(v.position);
            seen = true;
        }
        seen.then_some((lo, hi))
    }
}

/// A reversible record of one stitch primitive.
#[derive(Clone, Debug)]
pub enum StitchRecord {
    Split {
        edge: (EdgeId, Edge),
        e0: EdgeId,
        e1: EdgeId,
        e0_new: bool,
        e1_new: bool,
        mid: VertexId,
        faces: Vec<FaceId>,
    },
    Collapse {
        v: (VertexId, Vertex),
        e1: (EdgeId, Edge),
        e2: (EdgeId, Edge),
        n1: VertexId,
        n2: VertexId,
        new_edge: EdgeId,
        new_edge_new: bool,
        faces: Vec<FaceId>,
    },
    Merge {
        edge: (EdgeId, Edge),
        face_a: (FaceId, Face),
        face_b: (FaceId, Face),
        merged: FaceId,
    },
    MergePair {
        face_a: (FaceId, Face),
        face_b: (FaceId, Face),
        edges: Vec<(EdgeId, Edge)>,
        merged: FaceId,
    },
}

/// Unordered vertex pair key.
#[inline]
pub fn ukey(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

fn cycle_signature(lp: &[VertexId]) -> Vec<(VertexId, VertexId)> {
    let n = lp.len();
    let mut sig: Vec<(VertexId, VertexId)> = (0..n).map(|i| ukey(lp[i], lp[(i + 1) % n])).collect();
    sig.sort();
    sig.dedup();
    sig
}

#[inline]
fn push_unique<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

#[inline]
fn remove_first<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) {
    if let Some(i) = v.iter().position(|&y| y == x) {
        v.remove(i);
    }
}

fn shared_edge_index(lp: &[VertexId], v0: VertexId, v1: VertexId) -> Option<usize> {
    let n = lp.len();
    (0..n).find(|&i| {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        (a == v0 && b == v1) || (a == v1 && b == v0)
    })
}

/// Merge two vertex loops sharing the edge `{v0, v1}` into one, dropping
/// the shared edge. Both loops must carry the edge on their outer boundary
/// with consistent winding; `None` otherwise, or if a vertex would repeat.
pub fn splice_loops(
    loop_a: &[VertexId],
    loop_b: &[VertexId],
    v0: VertexId,
    v1: VertexId,
) -> Option<Vec<VertexId>> {
    let ia = shared_edge_index(loop_a, v0, v1)?;
    let ib = shared_edge_index(loop_b, v0, v1)?;
    let (na, nb) = (loop_a.len(), loop_b.len());
    let (u, w) = (loop_a[ia], loop_a[(ia + 1) % na]);
    if !(loop_b[ib] == w && loop_b[(ib + 1) % nb] == u) {
        return None;
    }
    let sa = (ia + 1) % na;
    let a_path: Vec<VertexId> = loop_a[sa..]
        .iter()
        .chain(loop_a[..sa].iter())
        .copied()
        .collect();
    let sb = (ib + 1) % nb;
    let b_path: Vec<VertexId> = loop_b[sb..]
        .iter()
        .chain(loop_b[..sb].iter())
        .copied()
        .collect();
    let merged: Vec<VertexId> = a_path[..a_path.len() - 1]
        .iter()
        .chain(b_path[..b_path.len() - 1].iter())
        .copied()
        .collect();
    let distinct: HashSet<VertexId> = merged.iter().copied().collect();
    if distinct.len() != merged.len() {
        return None;
    }
    Some(merged)
}

/// Trace a set of undirected boundary edges into closed vertex loops.
/// `None` if any vertex has valence ≠ 2 (a pinched / non-simple outline).
pub fn trace_loops(outline: &[(VertexId, VertexId)]) -> Option<Vec<Vec<VertexId>>> {
    let mut adj: IndexMap<VertexId, Vec<VertexId>> = IndexMap::new();
    let mut edges: IndexSet<(VertexId, VertexId)> = IndexSet::new();
    for &(u, w) in outline {
        if u == w {
            return None;
        }
        adj.entry(u).or_default().push(w);
        adj.entry(w).or_default().push(u);
        edges.insert(ukey(u, w));
    }
    if adj.values().any(|n| n.len() != 2) {
        return None;
    }
    // Start each loop from the outline's own direction (the first remaining
    // directed edge), so a traced loop keeps the winding of its faces.
    let mut loops = Vec::new();
    let directed: Vec<(VertexId, VertexId)> = outline.to_vec();
    let mut di = 0;
    while !edges.is_empty() {
        while di < directed.len() && !edges.contains(&ukey(directed[di].0, directed[di].1)) {
            di += 1;
        }
        let (a, b) = if di < directed.len() {
            directed[di]
        } else {
            *edges.first()?
        };
        edges.shift_remove(&ukey(a, b));
        let mut lp = vec![a, b];
        let (mut prev, mut cur) = (a, b);
        loop {
            let nbrs = &adj[&cur];
            let nxt = if nbrs[0] != prev { nbrs[0] } else { nbrs[1] };
            edges.shift_remove(&ukey(cur, nxt));
            if nxt == lp[0] {
                break;
            }
            lp.push(nxt);
            prev = cur;
            cur = nxt;
            if lp.len() > outline.len() + 1 {
                return None; // defensive: malformed adjacency
            }
        }
        loops.push(lp);
    }
    Some(loops)
}
