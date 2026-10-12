// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Topology helpers — graph queries over the scene's edge network.
//!
//! Used by tools to find polygons that close when a new edge is added: as
//! soon as a new edge completes a planar cycle in the edge graph — using any
//! combination of existing edges — that cycle becomes a face automatically.
//!
//! Position equality is tolerant: two vertices within `KEY_DECIMALS`
//! decimal places (≈ 0.1 mm at metric scale) are treated as the same node.

use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::{IndexMap, IndexSet};

use crate::geometry::{PolyFace, Segment};
use crate::math::{DVec3, VecExt, MIN_FACE_AREA};
use crate::mesh::{key as wkey, EdgeId, FaceId, Key, Mesh, VertexId, STITCH_TOL};
use crate::triangulate::{plane_axes, triangulate};

pub const PLANAR_TOLERANCE: f64 = 1e-3;
/// Two points closer than this weld together; also the gate for deciding a
/// crossing is real.
pub const SPLIT_TOLERANCE: f64 = 1e-4;

/// Above this many faces the overlap heal is skipped entirely.
pub const HEAL_FACE_CAP: usize = 3000;

/// The tolerant position key (same partition as the mesh weld).
#[inline]
pub fn tkey(p: DVec3) -> Key {
    wkey(p)
}

/// Whether two points coincide within the welding tolerance (≈ 0.1 mm).
#[inline]
pub fn same_position(p: DVec3, q: DVec3) -> bool {
    tkey(p) == tkey(q)
}

/// An existing segment whose endpoints coincide with `a`–`b`, by index.
pub fn find_duplicate_edge(edges: &[Segment], a: DVec3, b: DVec3) -> Option<usize> {
    let (ka, kb) = (tkey(a), tkey(b));
    if ka == kb {
        return None;
    }
    edges.iter().position(|e| {
        let (ea, eb) = (tkey(e.a), tkey(e.b));
        ea != eb && ((ea == ka && eb == kb) || (ea == kb && eb == ka))
    })
}

/// Smallest simple cycle in the edge graph that contains segment `a-b`.
///
/// The segment is *virtual*. Returns the cycle as vertices starting at `a`
/// and walking back to `b` through existing edges.
pub fn find_smallest_cycle_through(
    edges: &[Segment],
    a: DVec3,
    b: DVec3,
    max_len: usize,
) -> Option<Vec<DVec3>> {
    let (ka, kb) = (tkey(a), tkey(b));
    if ka == kb {
        return None;
    }
    let mut adj: IndexMap<Key, Vec<(Key, DVec3)>> = IndexMap::new();
    for e in edges {
        let (ea, eb) = (tkey(e.a), tkey(e.b));
        if ea == eb {
            continue;
        }
        if (ea == ka && eb == kb) || (ea == kb && eb == ka) {
            continue;
        }
        adj.entry(ea).or_default().push((eb, e.b));
        adj.entry(eb).or_default().push((ea, e.a));
    }
    if !adj.contains_key(&ka) || !adj.contains_key(&kb) {
        return None;
    }
    let mut parent: HashMap<Key, Option<Key>> = HashMap::new();
    let mut parent_pos: HashMap<Key, DVec3> = HashMap::new();
    parent.insert(kb, None);
    parent_pos.insert(kb, b);
    let mut q = VecDeque::from([kb]);
    let mut found = false;
    while let Some(u) = q.pop_front() {
        if u == ka {
            found = true;
            break;
        }
        if let Some(nbrs) = adj.get(&u) {
            for &(vk, vp) in nbrs {
                if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(vk) {
                    e.insert(Some(u));
                    parent_pos.insert(vk, vp);
                    q.push_back(vk);
                }
            }
        }
    }
    if !found {
        return None;
    }
    let mut path = Vec::new();
    let mut cur = Some(ka);
    while let Some(c) = cur {
        path.push(parent_pos[&c]);
        cur = parent[&c];
    }
    if path.len() < 3 || path.len() > max_len {
        return None;
    }
    Some(path)
}

/// Whether `vertices` all lie on a common plane within `tolerance`.
pub fn is_planar(vertices: &[DVec3], tolerance: f64) -> bool {
    let n = vertices.len();
    if n < 3 {
        return false;
    }
    if n == 3 {
        let e1 = vertices[1] - vertices[0];
        let e2 = vertices[2] - vertices[0];
        return e1.cross(e2).length() > 1e-6;
    }
    let v0 = vertices[0];
    let mut plane_normal = None;
    'search: for i in 1..n - 1 {
        for j in i + 1..n {
            let cross = (vertices[i] - v0).cross(vertices[j] - v0);
            if cross.length() > 1e-6 {
                plane_normal = Some(cross.unit_or_zero());
                break 'search;
            }
        }
    }
    let Some(pn) = plane_normal else { return false };
    vertices.iter().all(|&v| pn.dot(v - v0).abs() <= tolerance)
}

/// Where segment `p1-p2` meets segment `p3-p4` in 3D, or `None`.
///
/// Closest-points solution: accepts the hit only when the lines are not
/// parallel, their closest approach is within `tol`, and both parameters
/// land on their segment (endpoints included). Returns the midpoint of the
/// closest approach. Collinear overlaps return `None`.
pub fn segment_intersection(p1: DVec3, p2: DVec3, p3: DVec3, p4: DVec3, tol: f64) -> Option<DVec3> {
    let d1 = p2 - p1;
    let d2 = p4 - p3;
    let len1 = d1.length();
    let len2 = d2.length();
    if len1 < tol || len2 < tol {
        return None;
    }
    // Unit directions: guide lines are ±10 km segments and raw vectors lose
    // the determinant to cancellation.
    let u1 = d1 / len1;
    let u2 = d2 / len2;
    let a = u1.dot(u1);
    let b = u1.dot(u2);
    let c = u2.dot(u2);
    let w = p1 - p3;
    let d = u1.dot(w);
    let e = u2.dot(w);
    let denom = a * c - b * b;
    if denom < 1e-12 {
        return None; // parallel or collinear
    }
    let s = (b * e - c * d) / denom;
    let t = (a * e - b * d) / denom;
    let margin1 = tol / len1;
    let margin2 = tol / len2;
    if !(-margin1..=1.0 + margin1).contains(&(s / len1)) {
        return None;
    }
    if !(-margin2..=1.0 + margin2).contains(&(t / len2)) {
        return None;
    }
    let pt1 = p1 + u1 * s;
    let pt2 = p3 + u2 * t;
    let gap = (pt1 - pt2).length();
    let distance_tol = tol.max(1.0e-7 * len1.max(len2));
    if gap > distance_tol {
        return None; // skew: lines pass without meeting
    }
    Some((pt1 + pt2) * 0.5)
}

fn order_along(a: DVec3, b: DVec3, points: &[DVec3]) -> Vec<DVec3> {
    let d = b - a;
    let mut uniq: Vec<DVec3> = Vec::new();
    for &p in points {
        if (p - a).length() <= SPLIT_TOLERANCE || (p - b).length() <= SPLIT_TOLERANCE {
            continue;
        }
        if !uniq.iter().any(|q| (p - *q).length() <= SPLIT_TOLERANCE) {
            uniq.push(p);
        }
    }
    uniq.sort_by(|p, q| {
        (*p - a)
            .dot(d)
            .partial_cmp(&(*q - a).dot(d))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    uniq
}

/// The splits caused by adding a segment to a set of edges
/// ([`plan_edge_split`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeSplitPlan {
    /// The new edge broken at every interior crossing, ordered a→b.
    pub new_segments: Vec<(DVec3, DVec3)>,
    /// Per existing edge (by index), the interior point where the new edge
    /// crosses it.
    pub edge_cuts: Vec<(usize, DVec3)>,
}

/// Plan the splits caused by adding segment `a-b` to `edges`.
pub fn plan_edge_split(edges: &[Segment], a: DVec3, b: DVec3) -> EdgeSplitPlan {
    let mut new_cuts = Vec::new();
    let mut edge_cuts = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        let Some(mut point) = segment_intersection(a, b, e.a, e.b, SPLIT_TOLERANCE) else {
            continue;
        };
        // Snap a graze to the vertex it grazes.
        for anchor in [e.a, e.b, a, b] {
            if (point - anchor).length() <= SPLIT_TOLERANCE {
                point = anchor;
                break;
            }
        }
        if !(same_position(point, a) || same_position(point, b)) {
            new_cuts.push(point);
        }
        if !(same_position(point, e.a) || same_position(point, e.b)) {
            edge_cuts.push((i, point));
        }
    }
    let ordered = order_along(a, b, &new_cuts);
    let mut chain = vec![a];
    chain.extend(ordered);
    chain.push(b);
    EdgeSplitPlan {
        new_segments: chain.windows(2).map(|w| (w[0], w[1])).collect(),
        edge_cuts,
    }
}

/// Whether a face with the same vertex set as `cycle` already exists.
pub fn face_exists<'a>(faces: impl IntoIterator<Item = &'a [DVec3]>, cycle: &[DVec3]) -> bool {
    let ck: HashSet<Key> = cycle.iter().map(|&v| tkey(v)).collect();
    faces
        .into_iter()
        .any(|f| f.iter().map(|&v| tkey(v)).collect::<HashSet<Key>>() == ck)
}

// ---- Containment (face split / hole punching) ------------------------------

pub type P2 = (f64, f64);

pub fn on_segment_2d(p: P2, a: P2, b: P2, tol: f64) -> bool {
    let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    if cross.abs() > tol {
        return false;
    }
    let dot = (p.0 - a.0) * (b.0 - a.0) + (p.1 - a.1) * (b.1 - a.1);
    if dot < -tol {
        return false;
    }
    let sqlen = (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2);
    dot <= sqlen + tol
}

/// Ray-cast point-in-polygon, strict: points on the boundary are *not*
/// inside.
pub fn strictly_inside_2d(p: P2, poly: &[P2]) -> bool {
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    let mut inside = false;
    for i in 0..n {
        if on_segment_2d(p, poly[i], poly[j], 1e-7) {
            return false;
        }
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) {
            let xint = xi + (xj - xi) * (p.1 - yi) / (yj - yi);
            if p.0 < xint {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// A projector to a face's 2D plane.
#[derive(Clone, Copy)]
pub struct PlaneProj {
    pub origin: DVec3,
    pub u: DVec3,
    pub w: DVec3,
}

impl PlaneProj {
    pub fn new(origin: DVec3, normal: DVec3) -> Self {
        let (u, w) = plane_axes(normal);
        PlaneProj { origin, u, w }
    }

    #[inline]
    pub fn p(&self, p: DVec3) -> P2 {
        let rel = p - self.origin;
        (rel.dot(self.u), rel.dot(self.w))
    }

    pub fn poly(&self, pts: &[DVec3]) -> Vec<P2> {
        pts.iter().map(|&p| self.p(p)).collect()
    }
}

/// Whether `lp` lies entirely, strictly inside coplanar face `mother`.
/// With `outside_holes` the mother's holes count as *not hers*.
pub fn loop_inside_face(mother: &PolyFace, lp: &[DVec3], outside_holes: bool) -> bool {
    if mother.vertices.len() < 3 || lp.len() < 3 {
        return false;
    }
    let normal = mother.normal();
    let origin = mother.vertices[0];
    if lp
        .iter()
        .any(|&v| normal.dot(v - origin).abs() > PLANAR_TOLERANCE)
    {
        return false;
    }
    let pr = PlaneProj::new(origin, normal);
    let poly2 = pr.poly(&mother.vertices);
    if !lp.iter().all(|&v| strictly_inside_2d(pr.p(v), &poly2)) {
        return false;
    }
    if outside_holes {
        for hole in &mother.holes {
            let hole2 = pr.poly(hole);
            if lp.iter().any(|&v| strictly_inside_2d(pr.p(v), &hole2)) {
                return false;
            }
        }
    }
    true
}

/// Smallest existing face that strictly contains `lp` (hole-aware), by
/// vertex count.
pub fn find_containing_face(
    mesh: &Mesh,
    faces: &[FaceId],
    lp: &[DVec3],
    exclude: Option<FaceId>,
) -> Option<FaceId> {
    let mut best: Option<(FaceId, usize)> = None;
    for &f in faces {
        if Some(f) == exclude {
            continue;
        }
        let pf = mesh.poly_face(f);
        if loop_inside_face(&pf, lp, true) {
            let n = pf.vertices.len();
            if best.is_none_or(|(_, bn)| n < bn) {
                best = Some((f, n));
            }
        }
    }
    best.map(|(f, _)| f)
}

pub fn loop_edges(lp: &[DVec3]) -> Vec<(Key, Key)> {
    let n = lp.len();
    (0..n)
        .map(|i| ukey(tkey(lp[i]), tkey(lp[(i + 1) % n])))
        .collect()
}

#[inline]
fn ukey(a: Key, b: Key) -> (Key, Key) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Edges incident to any of `vertices` that border no face.
pub fn orphaned_edges_at(mesh: &Mesh, vertices: &[DVec3]) -> Vec<EdgeId> {
    let vkeys: HashSet<Key> = vertices.iter().map(|&v| tkey(v)).collect();
    let mut face_edges: HashSet<(Key, Key)> = HashSet::new();
    for (fid, _) in mesh.faces.iter() {
        face_edges.extend(loop_edges(&mesh.face_positions(fid)));
        for h in mesh.face_hole_positions(fid) {
            face_edges.extend(loop_edges(&h));
        }
    }
    let mut out = Vec::new();
    for (eid, _) in mesh.edges.iter() {
        let (a, b) = mesh.edge_ab(eid);
        let (ka, kb) = (tkey(a), tkey(b));
        if (vkeys.contains(&ka) || vkeys.contains(&kb)) && !face_edges.contains(&ukey(ka, kb)) {
            out.push(eid);
        }
    }
    out
}

pub fn faces_coplanar(n1: DVec3, n2: DVec3) -> bool {
    n1.unit_or_zero().dot(n2.unit_or_zero()).abs() > 0.999
}

fn face_all_edges(face: &PolyFace) -> HashSet<(Key, Key)> {
    let mut s: HashSet<(Key, Key)> = loop_edges(&face.vertices).into_iter().collect();
    for h in &face.holes {
        s.extend(loop_edges(h));
    }
    s
}

/// Whether `pt` lies on segment `p`–`q`, endpoints included.
pub fn point_on_seg_incl(pt: DVec3, p: DVec3, q: DVec3, tol: f64) -> bool {
    let pq = q - p;
    let length = pq.length();
    if length < tol {
        return same_position(pt, p);
    }
    let t = (pt - p).dot(pq) / (length * length);
    if t < -tol || t > 1.0 + tol {
        return false;
    }
    (pt - (p + pq * t)).length() < tol
}

/// Whether segment `a`–`b` lies on a boundary edge of `face` (outer loop or
/// any hole).
pub fn segment_on_face_boundary(a: DVec3, b: DVec3, face: &PolyFace) -> bool {
    for lp in std::iter::once(&face.vertices).chain(face.holes.iter()) {
        let n = lp.len();
        for i in 0..n {
            let (p, q) = (lp[i], lp[(i + 1) % n]);
            if point_on_seg_incl(a, p, q, SPLIT_TOLERANCE)
                && point_on_seg_incl(b, p, q, SPLIT_TOLERANCE)
            {
                return true;
            }
        }
    }
    false
}

/// Insert any `points` that lie on an edge's interior into `lp`.
pub fn refine_loop_with_points(lp: &[DVec3], points: &[DVec3], tol: f64) -> Vec<DVec3> {
    let mut out = Vec::new();
    let n = lp.len();
    for i in 0..n {
        let a = lp[i];
        let b = lp[(i + 1) % n];
        out.push(a);
        let ab = b - a;
        let length = ab.length();
        if length < tol {
            continue;
        }
        let mut on: Vec<(f64, DVec3)> = Vec::new();
        for &p in points {
            if same_position(p, a) || same_position(p, b) {
                continue;
            }
            let t = (p - a).dot(ab) / (length * length);
            if t <= tol / length || t >= 1.0 - tol / length {
                continue;
            }
            if (p - (a + ab * t)).length() < tol {
                on.push((t, p));
            }
        }
        on.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        out.extend(on.into_iter().map(|(_, p)| p));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushEdgeKind {
    Coplanar,
    Perp,
    Free,
}

/// How a push/pull side edge `a`–`b` of `face` attaches to the model:
/// a coplanar neighbour sharing it, a non-coplanar face carrying it, or
/// nothing.
pub fn classify_push_edge(
    mesh: &Mesh,
    face: FaceId,
    a: DVec3,
    b: DVec3,
    faces: &[FaceId],
) -> (PushEdgeKind, Option<FaceId>) {
    let fnm = mesh.face_normal(face);
    let eset = ukey(tkey(a), tkey(b));
    for &g in faces {
        if g == face {
            continue;
        }
        if faces_coplanar(fnm, mesh.face_normal(g)) {
            let pg = mesh.poly_face(g);
            if face_all_edges(&pg).contains(&eset) || segment_on_face_boundary(a, b, &pg) {
                return (PushEdgeKind::Coplanar, Some(g));
            }
        }
    }
    for &s in faces {
        if s == face || faces_coplanar(fnm, mesh.face_normal(s)) {
            continue;
        }
        if segment_on_face_boundary(a, b, &mesh.poly_face(s)) {
            return (PushEdgeKind::Perp, Some(s));
        }
    }
    (PushEdgeKind::Free, None)
}

/// Whether every boundary edge of `face` is also an edge of some other face.
pub fn face_is_bordered(mesh: &Mesh, face: FaceId, faces: &[FaceId]) -> bool {
    let base_edges = loop_edges(&mesh.face_positions(face));
    if base_edges.is_empty() {
        return false;
    }
    let mut others: HashSet<(Key, Key)> = HashSet::new();
    for &f in faces {
        if f == face {
            continue;
        }
        others.extend(loop_edges(&mesh.face_positions(f)));
        for h in mesh.face_hole_positions(f) {
            others.extend(loop_edges(&h));
        }
    }
    base_edges.iter().all(|e| others.contains(e))
}

// ---- Chord split -----------------------------------------------------------

fn point_on_segment_3d(p: DVec3, a: DVec3, b: DVec3, tol: f64) -> bool {
    let ab = b - a;
    let length = ab.length();
    if length < tol {
        return false;
    }
    let t = (p - a).dot(ab) / (length * length);
    if t < tol || t > 1.0 - tol {
        return false;
    }
    (p - (a + ab * t)).length() < tol
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopLoc {
    Vertex(usize),
    Edge(usize),
}

/// Where `p` sits on a face's boundary loop.
pub fn locate_on_loop(vertices: &[DVec3], p: DVec3) -> Option<LoopLoc> {
    let kp = tkey(p);
    if let Some(i) = vertices.iter().position(|&v| tkey(v) == kp) {
        return Some(LoopLoc::Vertex(i));
    }
    let n = vertices.len();
    (0..n)
        .find(|&i| point_on_segment_3d(p, vertices[i], vertices[(i + 1) % n], SPLIT_TOLERANCE))
        .map(LoopLoc::Edge)
}

fn augment(vertices: &[DVec3], on_edge: &IndexMap<usize, Vec<DVec3>>) -> Vec<DVec3> {
    let mut aug = Vec::new();
    for (i, &v) in vertices.iter().enumerate() {
        aug.push(v);
        if let Some(pts) = on_edge.get(&i) {
            let mut pts = pts.clone();
            pts.sort_by(|a, b| {
                (*a - v)
                    .length()
                    .partial_cmp(&(*b - v).length())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            aug.extend(pts);
        }
    }
    aug
}

/// If segment `a`–`b` is a chord of `face`, the two sub-loops it divides
/// the face into. Faces with holes are skipped.
pub fn split_face_by_chord(
    face: &PolyFace,
    a: DVec3,
    b: DVec3,
) -> Option<(Vec<DVec3>, Vec<DVec3>)> {
    if !face.holes.is_empty() || face.vertices.len() < 3 {
        return None;
    }
    let la = locate_on_loop(&face.vertices, a)?;
    let lb = locate_on_loop(&face.vertices, b)?;
    let mut on_edge: IndexMap<usize, Vec<DVec3>> = IndexMap::new();
    if let LoopLoc::Edge(i) = la {
        on_edge.entry(i).or_default().push(a);
    }
    if let LoopLoc::Edge(i) = lb {
        on_edge.entry(i).or_default().push(b);
    }
    let aug = augment(&face.vertices, &on_edge);
    let keys: Vec<Key> = aug.iter().map(|&v| tkey(v)).collect();
    let mut ia = keys.iter().position(|&k| k == tkey(a))?;
    let mut ib = keys.iter().position(|&k| k == tkey(b))?;
    if ia > ib {
        std::mem::swap(&mut ia, &mut ib);
    }
    let m = aug.len();
    if ib - ia <= 1 || (ia == 0 && ib == m - 1) {
        return None;
    }
    let pr = PlaneProj::new(face.vertices[0], face.normal());
    let mid = (a + b) * 0.5;
    let poly2 = pr.poly(&face.vertices);
    if !strictly_inside_2d(pr.p(mid), &poly2) {
        return None;
    }
    let loop_a: Vec<DVec3> = aug[ia..=ib].to_vec();
    let loop_b: Vec<DVec3> = aug[ib..].iter().chain(aug[..=ia].iter()).copied().collect();
    if loop_a.len() < 3 || loop_b.len() < 3 {
        return None;
    }
    Some((loop_a, loop_b))
}

// ---- Loop subtraction --------------------------------------------------------

fn face_plane_proj(face: &PolyFace) -> (PlaneProj, Vec<P2>) {
    let pr = PlaneProj::new(face.vertices[0], face.normal());
    let poly = pr.poly(&face.vertices);
    (pr, poly)
}

/// If `lp` shares a contiguous boundary arc with `face` and pushes a single
/// run of vertices through its interior, that interior chain `[P, …, Q]`.
pub fn find_subdividing_chain(face: &PolyFace, lp: &[DVec3]) -> Option<Vec<DVec3>> {
    if face.vertices.len() < 3 || lp.len() < 3 {
        return None;
    }
    let (pr, poly2) = face_plane_proj(face);
    let mut labels: Vec<bool> = Vec::new(); // true = "in", false = "bdry"
    for &v in lp {
        if locate_on_loop(&face.vertices, v).is_some() {
            labels.push(false);
        } else if strictly_inside_2d(pr.p(v), &poly2) {
            labels.push(true);
        } else {
            return None;
        }
    }
    if !labels.contains(&true) || !labels.contains(&false) {
        return None;
    }
    let n = lp.len();
    let start = labels.iter().position(|&l| !l)?;
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for k in 0..n {
        let idx = (start + k) % n;
        if labels[idx] {
            cur.push(idx);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    if runs.len() != 1 {
        return None;
    }
    let run = &runs[0];
    let p = lp[(run[0] + n - 1) % n];
    let q = lp[(run[run.len() - 1] + 1) % n];
    if same_position(p, q) {
        return None;
    }
    let mut chain = vec![p];
    chain.extend(run.iter().map(|&i| lp[i]));
    chain.push(q);
    Some(chain)
}

fn arc<T: Clone>(seq: &[T], i: usize, j: usize) -> Vec<T> {
    if i <= j {
        seq[i..=j].to_vec()
    } else {
        seq[i..].iter().chain(seq[..=j].iter()).cloned().collect()
    }
}

/// Split `face` along `chain` (`[P, …interior…, Q]`) into its two sub-loops.
pub fn split_face_by_chain(
    face_vertices: &[DVec3],
    chain: &[DVec3],
) -> Option<(Vec<DVec3>, Vec<DVec3>)> {
    let p = chain[0];
    let q = chain[chain.len() - 1];
    let middles: Vec<DVec3> = chain[1..chain.len() - 1].to_vec();
    let mut on_edge: IndexMap<usize, Vec<DVec3>> = IndexMap::new();
    for pt in [p, q] {
        match locate_on_loop(face_vertices, pt)? {
            LoopLoc::Edge(i) => on_edge.entry(i).or_default().push(pt),
            LoopLoc::Vertex(_) => {}
        }
    }
    let aug = augment(face_vertices, &on_edge);
    let keys: Vec<Key> = aug.iter().map(|&x| tkey(x)).collect();
    let ip = keys.iter().position(|&k| k == tkey(p))?;
    let iq = keys.iter().position(|&k| k == tkey(q))?;
    let mut region_pq = arc(&aug, ip, iq);
    region_pq.extend(middles.iter().rev().copied());
    let mut region_qp = arc(&aug, iq, ip);
    region_qp.extend(middles.iter().copied());
    if region_pq.len() < 3 || region_qp.len() < 3 {
        return None;
    }
    Some((region_pq, region_qp))
}

fn key_set(pts: &[DVec3]) -> HashSet<Key> {
    pts.iter().map(|&v| tkey(v)).collect()
}

/// The remainder of `face` after `lp` is carved out of it (corner / edge
/// rectangle). `None` if it isn't that case.
pub fn subtract_loop_from_face(face: &PolyFace, lp: &[DVec3]) -> Option<Vec<DVec3>> {
    let chain = find_subdividing_chain(face, lp)?;
    let (r1, r2) = split_face_by_chain(&face.vertices, &chain)?;
    let lk = key_set(lp);
    if key_set(&r1) == lk {
        return Some(r2);
    }
    if key_set(&r2) == lk {
        return Some(r1);
    }
    None
}

/// The pieces of `face` left around `lp` when EVERY corner of the loop sits
/// on the face's boundary — a band drawn across a face. `None` when this is
/// not that case.
pub fn carve_loop_by_chords(face: &PolyFace, lp: &[DVec3]) -> Option<Vec<Vec<DVec3>>> {
    if face.vertices.len() < 3 || lp.len() < 3 {
        return None;
    }
    if lp
        .iter()
        .any(|&v| locate_on_loop(&face.vertices, v).is_none())
    {
        return None;
    }
    let (pr, poly2) = face_plane_proj(face);
    let mut chords = Vec::new();
    let n = lp.len();
    for i in 0..n {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        let mid = (a + b) * 0.5;
        if locate_on_loop(&face.vertices, mid).is_some() {
            continue;
        }
        if !strictly_inside_2d(pr.p(mid), &poly2) {
            return None;
        }
        chords.push([a, b]);
    }
    if chords.is_empty() {
        return None;
    }
    let mut pieces: Vec<Vec<DVec3>> = vec![face.vertices.clone()];
    for chord in &chords {
        let mid = (chord[0] + chord[1]) * 0.5;
        let mut done = false;
        for idx in 0..pieces.len() {
            let p2 = pr.poly(&pieces[idx]);
            if !strictly_inside_2d(pr.p(mid), &p2) {
                continue;
            }
            let (s0, s1) = split_face_by_chain(&pieces[idx], chord)?;
            pieces.splice(idx..idx + 1, [s0, s1]);
            done = true;
            break;
        }
        if !done {
            return None;
        }
    }
    let lk = key_set(lp);
    let total = pieces.len();
    let rest: Vec<Vec<DVec3>> = pieces.into_iter().filter(|p| key_set(p) != lk).collect();
    if rest.len() != total - 1 {
        return None;
    }
    Some(rest)
}

// ---- Multiple-cycle detection -------------------------------------------------

fn same_cycle(c1: &[DVec3], c2: &[DVec3]) -> bool {
    key_set(c1) == key_set(c2)
}

/// Up to `max_results` distinct minimal cycles through segment `a`–`b`.
pub fn find_cycles_through(
    edges: &[Segment],
    a: DVec3,
    b: DVec3,
    max_results: usize,
) -> Vec<Vec<DVec3>> {
    let Some(first) = find_smallest_cycle_through(edges, a, b, 32) else {
        return Vec::new();
    };
    let mut cycles = vec![first.clone()];
    if max_results >= 2 {
        let (ka, kb) = (tkey(a), tkey(b));
        let interior: HashSet<Key> = first
            .iter()
            .map(|&v| tkey(v))
            .filter(|k| *k != ka && *k != kb)
            .collect();
        if !interior.is_empty() {
            let filtered: Vec<Segment> = edges
                .iter()
                .filter(|e| !interior.contains(&tkey(e.a)) && !interior.contains(&tkey(e.b)))
                .copied()
                .collect();
            if let Some(second) = find_smallest_cycle_through(&filtered, a, b, 32) {
                if !same_cycle(&second, &first) {
                    cycles.push(second);
                }
            }
        }
    }
    cycles
}

// ---- Polygon offset ------------------------------------------------------------

pub fn offset_line_intersection(
    p0: DVec3,
    u: DVec3,
    p1: DVec3,
    v: DVec3,
    n: DVec3,
) -> Option<DVec3> {
    let denom = u.cross(v).dot(n);
    if denom.abs() < 1e-9 {
        return Some(p1);
    }
    let s = (p1 - p0).cross(v).dot(n) / denom;
    Some(p0 + u * s)
}

/// Offset a planar polygon `lp` by `d` in its plane: `d > 0` inward, `d < 0`
/// outward. `None` if the loop degenerates or the offset inverts it.
pub fn offset_loop(lp: &[DVec3], normal: DVec3, d: f64) -> Option<Vec<DVec3>> {
    let count = lp.len();
    if count < 3 {
        return None;
    }
    let n = normal.unit_or_zero();
    let mut lines = Vec::with_capacity(count);
    for i in 0..count {
        let a = lp[i];
        let b = lp[(i + 1) % count];
        let edge = b - a;
        if edge.length() < SPLIT_TOLERANCE {
            return None;
        }
        let e = edge.unit_or_zero();
        let inward = n.cross(e).unit_or_zero();
        lines.push((a + inward * d, e));
    }
    let mut new_loop = Vec::with_capacity(count);
    for i in 0..count {
        let (p0, u) = lines[(i + count - 1) % count];
        let (p1, v) = lines[i];
        new_loop.push(offset_line_intersection(p0, u, p1, v, n)?);
    }
    for i in 0..count {
        let orig = lp[(i + 1) % count] - lp[i];
        let new = new_loop[(i + 1) % count] - new_loop[i];
        if new.length() < SPLIT_TOLERANCE {
            return None;
        }
        if orig.unit_or_zero().dot(new.unit_or_zero()) <= 0.0 {
            return None;
        }
    }
    Some(new_loop)
}

/// The largest offset in the direction of `sign` that `offset_loop` still
/// accepts (0.0 when even a hair collapses the loop). Bisection.
pub fn max_offset_distance(lp: &[DVec3], normal: DVec3, sign: f64, limit: f64) -> f64 {
    let step = limit.abs();
    if offset_loop(lp, normal, sign * 1e-4).is_none() {
        return 0.0;
    }
    let (mut lo, mut hi) = (1e-4, step);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if offset_loop(lp, normal, sign * mid).is_none() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

// ---- Heal overlapping coplanar faces -------------------------------------------

/// Whether `inner` lies inside `outer` (coplanar loops, by centroid, then a
/// guaranteed interior point).
pub fn loop_inside_loop(inner: &[DVec3], outer: &[DVec3], normal: DVec3) -> bool {
    if inner.len() < 3 || outer.len() < 3 {
        return false;
    }
    let pr = PlaneProj::new(outer[0], normal);
    let poly = pr.poly(outer);
    let c = inner.iter().copied().sum::<DVec3>() / inner.len() as f64;
    if point_inside_2d(pr.p(c), &poly) {
        return true;
    }
    match interior_point(inner, normal) {
        Some(point) => point_inside_2d(pr.p(point), &poly),
        None => false,
    }
}

/// A point genuinely inside `lp`: the centroid of its biggest triangle.
pub fn interior_point(lp: &[DVec3], normal: DVec3) -> Option<DVec3> {
    let tris = triangulate(lp, &[], Some(normal));
    let mut best = None;
    let mut best_area = 0.0;
    for [a, b, c] in tris {
        let area = (b - a).cross(c - a).length_squared();
        if area > best_area {
            best_area = area;
            best = Some((a + b + c) / 3.0);
        }
    }
    best
}

pub fn point_inside_2d(pt: P2, poly: &[P2]) -> bool {
    let (x, y) = pt;
    let mut inside = false;
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Drop holes nested inside a larger hole of the same face.
pub fn maximal_holes(holes: &[Vec<DVec3>]) -> Vec<Vec<DVec3>> {
    let area = |lp: &[DVec3]| -> f64 {
        let n = lp.len();
        let mut a = 0.0;
        for i in 0..n {
            a += lp[i].x * lp[(i + 1) % n].y;
            a -= lp[(i + 1) % n].x * lp[i].y;
        }
        a.abs() / 2.0
    };
    let areas: Vec<f64> = holes.iter().map(|h| area(h)).collect();
    let mut keep = Vec::new();
    for (i, h) in holes.iter().enumerate() {
        if h.len() < 3 {
            keep.push(h.clone());
            continue;
        }
        let nrm = (h[1] - h[0]).cross(h[2] - h[0]);
        if nrm.length() < 1e-9 {
            keep.push(h.clone());
            continue;
        }
        let nrm = nrm.unit_or_zero();
        let nested = (0..holes.len())
            .any(|j| j != i && areas[j] > areas[i] + 1e-6 && loop_inside_loop(h, &holes[j], nrm));
        if !nested {
            keep.push(h.clone());
        }
    }
    keep
}

fn g_inside_a_hole(face: &PolyFace, lp: &[DVec3]) -> bool {
    let n = face.normal();
    face.holes
        .iter()
        .any(|h| h.len() >= 3 && loop_inside_loop(lp, h, n))
}

/// Whether `point` lies in `face`'s solid region.
pub fn point_in_face_solid(face: &PolyFace, point: DVec3) -> bool {
    let pr = PlaneProj::new(face.vertices[0], face.normal());
    let pt = pr.p(point);
    if !point_inside_2d(pt, &pr.poly(&face.vertices)) {
        return false;
    }
    !face.holes.iter().any(|h| point_inside_2d(pt, &pr.poly(h)))
}

/// How the faces of a flat drawing face BEFORE an edit.
#[derive(Clone)]
pub struct FootprintEntry {
    pub face: FaceId,
    pub normal: DVec3,
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub lo: DVec3,
    pub hi: DVec3,
}

pub type Footprint = Vec<FootprintEntry>;

/// The footprint of a flat drawing, or `None` where the orientation pass
/// never runs (3D, or past the heal cap).
pub fn winding_footprint(mesh: &Mesh) -> Option<Footprint> {
    if mesh.faces.len() > HEAL_FACE_CAP || !mesh_is_flat(mesh) {
        return None;
    }
    let mut out = Vec::new();
    for (f, _) in mesh.faces.iter() {
        let outer = mesh.face_positions(f);
        if outer.is_empty() {
            continue;
        }
        let lo = outer
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
        let hi = outer
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
        out.push(FootprintEntry {
            face: f,
            normal: mesh.face_normal(f),
            outer,
            holes: mesh.face_hole_positions(f),
            lo,
            hi,
        });
    }
    Some(out)
}

fn footprint_normal(footprint: &Footprint, mesh: &Mesh, face: FaceId) -> Option<DVec3> {
    let n = mesh.face_normal(face);
    let point = interior_point(&mesh.face_positions(face), n)?;
    let t = PLANAR_TOLERANCE;
    for e in footprint {
        if !(e.lo.x - t <= point.x
            && point.x <= e.hi.x + t
            && e.lo.y - t <= point.y
            && point.y <= e.hi.y + t
            && e.lo.z - t <= point.z
            && point.z <= e.hi.z + t)
        {
            continue;
        }
        if e.normal.dot(n).abs() < 0.999 {
            continue;
        }
        if (point - e.outer[0]).dot(e.normal).abs() > PLANAR_TOLERANCE {
            continue;
        }
        let pr = PlaneProj::new(e.outer[0], e.normal);
        let pt = pr.p(point);
        if point_inside_2d(pt, &pr.poly(&e.outer))
            && !e.holes.iter().any(|h| point_inside_2d(pt, &pr.poly(h)))
        {
            return Some(e.normal);
        }
    }
    None
}

fn round2(x: f64) -> i64 {
    crate::math::round_even_int(x * 100.0)
}

/// Flip faces whose winding came out reversed, so coplanar faces face the
/// same way. With a `footprint`, faces that were already there keep their
/// winding and a face cut from another faces the way it did. Returns the
/// flipped faces (they are re-created, so the ids returned are the old ones).
pub fn orient_coplanar_faces(mesh: &mut Mesh, footprint: Option<&Footprint>) -> Vec<FaceId> {
    // Keyed by (rounded |normal|, rounded distance from the origin).
    type PlaneKey = ((i64, i64, i64), i64);
    let mut groups: IndexMap<PlaneKey, Vec<FaceId>> = IndexMap::new();
    for (f, _) in mesh.faces.iter() {
        let n = mesh.face_normal(f);
        let axis = (round2(n.x.abs()), round2(n.y.abs()), round2(n.z.abs()));
        let dist = round2(n.dot(mesh.face_centroid(f)).abs());
        groups.entry((axis, dist)).or_default().push(f);
    }
    let owned: HashSet<FaceId> = footprint
        .map(|fp| fp.iter().map(|e| e.face).collect())
        .unwrap_or_default();
    let mut flipped = Vec::new();
    for (_, fs) in groups {
        if fs.len() < 2 {
            continue;
        }
        let mut dominant = DVec3::ZERO;
        for &f in &fs {
            dominant += mesh.face_normal(f) * mesh.face_area(f);
        }
        for &f in &fs {
            let mut target = dominant;
            if let Some(fp) = footprint {
                if owned.contains(&f) {
                    continue; // the user's, as it stands
                }
                if let Some(old) = footprint_normal(fp, mesh, f) {
                    target = old;
                }
            }
            if mesh.face_normal(f).dot(target) < 0.0 {
                let mut outer = mesh.face_positions(f);
                outer.reverse();
                let holes = mesh.face_hole_positions(f);
                let attrs = mesh.face(f).attrs.clone();
                mesh.remove_face(f);
                let nf = mesh.add_face(&outer, &holes);
                mesh.faces[nf].attrs = attrs;
                flipped.push(f);
            }
        }
    }
    flipped
}

fn collinear_overlapping(mesh: &Mesh, e1: EdgeId, e2: EdgeId) -> bool {
    let (a, b) = mesh.edge_ab(e1);
    let (c, d) = mesh.edge_ab(e2);
    let (d1, d2) = (b - a, d - c);
    if d1.length() < 1e-9 || d2.length() < 1e-9 {
        return false;
    }
    let u = d2.unit_or_zero();
    if d1.unit_or_zero().cross(u).length() > 1e-4 {
        return false;
    }
    let rel = a - c;
    if (rel - u * rel.dot(u)).length() > PLANAR_TOLERANCE {
        return false;
    }
    let ta = (a - c).dot(u);
    let tb = (b - c).dot(u);
    let (lo1, hi1) = (ta.min(tb), ta.max(tb));
    hi1.min(d2.length()) - lo1.max(0.0) > 1e-4
}

/// Split every edge that runs past a vertex belonging to another face, in
/// rounds until no T-junction is left. `seed_bounds` (an axis-aligned box)
/// limits the sweep to edges reaching it; `None` sweeps the whole mesh.
pub fn sweep_tjunctions(mesh: &mut Mesh, seed_bounds: Option<(DVec3, DVec3)>) {
    let tol = STITCH_TOL;
    let tol2 = tol * tol;
    let scope = seed_bounds.map(|(lo, hi)| (lo - DVec3::splat(tol), hi + DVec3::splat(tol)));
    loop {
        let verts: Vec<VertexId> = mesh.vertex_ids();
        let edges: Vec<EdgeId> = mesh.edge_ids();
        if verts.is_empty() || edges.is_empty() {
            break;
        }
        let vpos: Vec<DVec3> = verts.iter().map(|&v| mesh.pos(v)).collect();
        let slot: HashMap<VertexId, usize> =
            verts.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        // x-sorted vertex order for the broad phase.
        let mut order: Vec<usize> = (0..verts.len()).collect();
        order.sort_by(|&i, &j| {
            vpos[i]
                .x
                .partial_cmp(&vpos[j].x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let xs: Vec<f64> = order.iter().map(|&i| vpos[i].x).collect();
        let mut hits: Vec<(EdgeId, VertexId)> = Vec::new();
        for &e in &edges {
            let ed = mesh.edge(e);
            let (i0, i1) = (slot[&ed.v0], slot[&ed.v1]);
            let (va, vb) = (vpos[i0], vpos[i1]);
            let (elo, ehi) = (va.min(vb), va.max(vb));
            if let Some((slo, shi)) = scope {
                if !(ehi.x >= slo.x
                    && ehi.y >= slo.y
                    && ehi.z >= slo.z
                    && elo.x <= shi.x
                    && elo.y <= shi.y
                    && elo.z <= shi.z)
                {
                    continue;
                }
            }
            let seg = vb - va;
            let l2 = seg.length_squared();
            if l2 <= tol2 {
                continue;
            }
            let start = xs.partition_point(|&x| x < elo.x - tol);
            let end = xs.partition_point(|&x| x <= ehi.x + tol);
            let tol_t = tol / l2.sqrt();
            let mut best = usize::MAX;
            for &vi in &order[start..end.max(start)] {
                if vi == i0 || vi == i1 || vi >= best {
                    continue;
                }
                let p = vpos[vi];
                if p.y < elo.y - tol || p.y > ehi.y + tol || p.z < elo.z - tol || p.z > ehi.z + tol
                {
                    continue;
                }
                let rel = p - va;
                let t = rel.dot(seg) / l2;
                if !(t > tol_t && t < 1.0 - tol_t) {
                    continue;
                }
                let d = rel - seg * t;
                if d.length_squared() < tol2 {
                    best = vi;
                }
            }
            if best != usize::MAX {
                hits.push((e, verts[best]));
            }
        }
        if hits.is_empty() {
            break;
        }
        for (e, v) in hits {
            if mesh.has_edge(e) {
                mesh.split_edge_at(e, v);
            }
        }
    }
}

/// Split edges at T-junction vertices (whole mesh).
pub fn resolve_tjunctions(mesh: &mut Mesh) {
    sweep_tjunctions(mesh, None);
}

/// *Autofold*: split every face a move has warped out of its plane into
/// planar pieces along fold edges. Returns the faces folded (already removed).
pub fn fold_nonplanar_faces(mesh: &mut Mesh, tolerance: f64) -> Vec<FaceId> {
    let mut folded = Vec::new();
    for face in mesh.face_ids() {
        if !mesh.has_face(face) {
            continue;
        }
        let mut pts = mesh.face_positions(face);
        for h in mesh.face_hole_positions(face) {
            pts.extend(h);
        }
        if pts.len() <= 3 || is_planar(&pts, tolerance) {
            continue;
        }
        let tris = mesh.face_triangulate(face, None);
        if tris.len() < 2 {
            continue;
        }
        let attrs = mesh.face(face).attrs.clone();
        mesh.remove_face(face);
        let mut pieces: IndexSet<FaceId> = IndexSet::new();
        for [a, b, c] in tris {
            let p = mesh.add_face(&[a, b, c], &[]);
            mesh.faces[p].attrs = attrs.clone();
            pieces.insert(p);
        }
        let mut merged = true;
        while merged {
            merged = false;
            'scan: for f in pieces.clone() {
                if !mesh.has_face(f) {
                    pieces.shift_remove(&f);
                    continue;
                }
                let lp = mesh.face(f).outer.clone();
                for i in 0..lp.len() {
                    let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % lp.len()]) else {
                        continue;
                    };
                    let ef = mesh.edge(e).faces.clone();
                    if ef.len() != 2 {
                        continue;
                    }
                    let g = if ef[1] == f { ef[0] } else { ef[1] };
                    if g == f || !pieces.contains(&g) {
                        continue;
                    }
                    if mesh
                        .face_normal(f)
                        .unit_or_zero()
                        .dot(mesh.face_normal(g).unit_or_zero())
                        > 0.999
                    {
                        if let Some(region) = mesh.dissolve_edge(e) {
                            mesh.faces[region].attrs = attrs.clone();
                            pieces.shift_remove(&f);
                            pieces.shift_remove(&g);
                            pieces.insert(region);
                            merged = true;
                            break 'scan;
                        }
                    }
                }
            }
        }
        folded.push(face);
    }
    folded
}

/// Remove edges that bound no face and lie collinearly over another edge.
pub fn prune_collinear_orphan_edges(mesh: &mut Mesh) -> Vec<EdgeId> {
    let mut removed = Vec::new();
    let edges = mesh.edge_ids();
    let orphans: Vec<EdgeId> = edges
        .iter()
        .copied()
        .filter(|&e| mesh.edge(e).faces.is_empty())
        .collect();
    if orphans.is_empty() {
        return removed;
    }
    let boxes: Vec<(DVec3, DVec3)> = edges
        .iter()
        .map(|&e| {
            let (a, b) = mesh.edge_ab(e);
            (
                a.min(b) - DVec3::splat(PLANAR_TOLERANCE),
                a.max(b) + DVec3::splat(PLANAR_TOLERANCE),
            )
        })
        .collect();
    let slot: HashMap<EdgeId, usize> = edges.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let mut gone: HashSet<EdgeId> = HashSet::new();
    for e in orphans {
        let i = slot[&e];
        let (lo, hi) = boxes[i];
        for (j, &other) in edges.iter().enumerate() {
            let (olo, ohi) = boxes[j];
            if !(ohi.x >= lo.x
                && ohi.y >= lo.y
                && ohi.z >= lo.z
                && olo.x <= hi.x
                && olo.y <= hi.y
                && olo.z <= hi.z)
            {
                continue;
            }
            if other == e || gone.contains(&other) {
                continue;
            }
            if collinear_overlapping(mesh, e, other) {
                mesh.remove_edge(e);
                removed.push(e);
                gone.insert(e);
                break;
            }
        }
    }
    removed
}

/// Whether every face lies on one common plane — a flat 2D drawing.
pub fn mesh_is_flat(mesh: &Mesh) -> bool {
    let faces = mesh.face_ids();
    if faces.len() < 2 {
        return true;
    }
    let n0 = mesh.face_normal(faces[0]);
    let d0 = n0.dot(mesh.pos(mesh.face(faces[0]).outer[0]));
    for &f in &faces[1..] {
        if !faces_coplanar(n0, mesh.face_normal(f)) {
            return false;
        }
        if (n0.dot(mesh.pos(mesh.face(f).outer[0])) - d0).abs() > PLANAR_TOLERANCE {
            return false;
        }
    }
    true
}

/// Clean up coplanar face overlaps that draw/delete sequences can leave
/// (redundant nested holes, a redundant mother, reversed faces, partial
/// overlaps on flat drawings, volumetric winding in 3D). `partial`: `None`
/// = auto (only on a flat drawing). Returns the faces removed.
pub fn heal_overlapping_faces(
    mesh: &mut Mesh,
    coverage: f64,
    partial: Option<bool>,
    footprint: Option<&Footprint>,
) -> Vec<FaceId> {
    if mesh.faces.len() > HEAL_FACE_CAP {
        return Vec::new();
    }
    let flat = mesh_is_flat(mesh);
    let partial = partial.unwrap_or(flat);

    prune_collinear_orphan_edges(mesh);
    resolve_tjunctions(mesh);
    if flat {
        orient_coplanar_faces(mesh, footprint);
    }

    // 1. Dedupe nested holes.
    for face in mesh.face_ids() {
        if mesh.face(face).holes.len() < 2 {
            continue;
        }
        let holes = mesh.face_hole_positions(face);
        let keep = maximal_holes(&holes);
        if keep.len() != holes.len() {
            let outer = mesh.face_positions(face);
            let attrs = mesh.face(face).attrs.clone();
            mesh.remove_face(face);
            let nf = mesh.add_face(&outer, &keep);
            mesh.faces[nf].attrs = attrs;
        }
    }

    // 2. Remove a redundant mother covered by faces that aren't in its holes.
    let mut removed = Vec::new();
    let snap = mesh.face_ids();
    let polys: Vec<PolyFace> = snap.iter().map(|&f| mesh.poly_face(f)).collect();
    let nrm: Vec<DVec3> = snap.iter().map(|&f| mesh.face_normal(f)).collect();
    let area: Vec<f64> = snap.iter().map(|&f| mesh.face_area(f)).collect();
    let mut gone: HashSet<usize> = HashSet::new();
    for i in 0..snap.len() {
        let a_face = area[i];
        if a_face < MIN_FACE_AREA {
            continue;
        }
        let mut covered = 0.0;
        for j in 0..snap.len() {
            if !(nrm[j].dot(nrm[i]).abs() > 0.999 && area[j] < a_face) {
                continue;
            }
            if j == i || gone.contains(&j) {
                continue;
            }
            if loop_inside_face(&polys[i], &polys[j].vertices, false)
                && !g_inside_a_hole(&polys[i], &polys[j].vertices)
            {
                covered += area[j];
            }
        }
        if covered > coverage * a_face {
            mesh.remove_face(snap[i]);
            removed.push(snap[i]);
            gone.insert(i);
        }
    }

    // 3. (Flat plans) punch a matching hole for a partial overlap.
    if partial {
        let snap3 = mesh.face_ids();
        let polys3: Vec<PolyFace> = snap3.iter().map(|&f| mesh.poly_face(f)).collect();
        let n3: Vec<DVec3> = snap3.iter().map(|&f| mesh.face_normal(f)).collect();
        let a3: Vec<f64> = snap3.iter().map(|&f| mesh.face_area(f)).collect();
        let mut punch: IndexMap<usize, Vec<usize>> = IndexMap::new();
        for i3 in 0..snap3.len() {
            let tris = mesh.face_triangulate(snap3[i3], None);
            if tris.is_empty() {
                continue;
            }
            let probes: Vec<DVec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
            let a_face = a3[i3];
            for j3 in 0..snap3.len() {
                if !(n3[j3].dot(n3[i3]).abs() > 0.999 && a3[j3] > a_face) {
                    continue;
                }
                if j3 == i3 {
                    continue;
                }
                let g = &polys3[j3];
                if g_inside_a_hole(g, &polys3[i3].vertices) {
                    continue;
                }
                let inside = probes
                    .iter()
                    .filter(|&&p| point_in_face_solid(g, p))
                    .count();
                if inside * 2 > probes.len() {
                    punch.entry(j3).or_default().push(i3);
                    break;
                }
            }
        }
        for (g, smalls) in punch {
            let gf = snap3[g];
            if !mesh.has_face(gf) {
                continue;
            }
            let outer = polys3[g].vertices.clone();
            let mut holes = polys3[g].holes.clone();
            holes.extend(smalls.iter().map(|&s| polys3[s].vertices.clone()));
            let attrs = mesh.face(gf).attrs.clone();
            mesh.remove_face(gf);
            let nf = mesh.add_face(&outer, &holes);
            mesh.faces[nf].attrs = attrs;
        }
    }

    // 4. (3D) Volumetric winding.
    if !flat {
        crate::orient::orient_outward(mesh, crate::orient::DEFAULT_SEED, None);
    }
    removed
}
