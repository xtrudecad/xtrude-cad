// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Consistent outward orientation for closed solids in the non-manifold mesh.
//!
//! The root-fix invariant: every face of a closed solid is wound so its
//! normal points *outward* from the enclosed volume. Orientation is decided
//! **per face, independently**, by parity ray casting: a point just off a
//! face along its normal is *outside* the solid iff a ray from it crosses
//! the rest of the mesh an even number of times. That is robust to
//! non-manifold edges and needs no seed face.
//!
//! Only meaningful for a **closed** component (no boundary edges).

use std::collections::{HashMap, HashSet};

use crate::arrangement::{plane_basis, point_in_polygon};
use crate::math::{DVec3, VecExt};
use crate::mesh::{FaceId, Mesh};
use crate::mt19937::Mt19937;

const EPS: f64 = 1e-7;
const T_MIN: f64 = 1e-6;
/// Number of jittered rays voted over to step around degenerate hits.
const RAYS: usize = 7;
/// Small angular perturbation of the jittered rays (a wide cone
/// overshoots thin features far away).
const JITTER: f64 = 3e-4;

pub type Triangle = [DVec3; 3];

/// Möller–Trumbore: the ray parameter `t > 0` of the intersection, or None.
pub fn ray_triangle(origin: DVec3, direction: DVec3, tri: &Triangle) -> Option<f64> {
    let [a, b, c] = *tri;
    let e1 = b - a;
    let e2 = c - a;
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < EPS {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = origin - a;
    let u = tvec.dot(p) * inv;
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = direction.dot(q) * inv;
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > T_MIN).then_some(t)
}

fn basis(n: DVec3) -> (DVec3, DVec3) {
    let mut reference = DVec3::X;
    if reference.dot(n).abs() > 0.9 {
        reference = DVec3::Y;
    }
    let u = n.cross(reference).unit_or_zero();
    let v = n.cross(u).unit_or_zero();
    (u, v)
}

/// Triangles flattened for batched Möller–Trumbore, each tagged with its
/// face index so a query can exclude faces without repacking.
#[derive(Clone, Default)]
pub struct PackedTris {
    pub a: Vec<DVec3>,
    pub e1: Vec<DVec3>,
    pub e2: Vec<DVec3>,
    pub face_idx: Vec<usize>,
}

impl PackedTris {
    pub fn from_lists(lists: &[Vec<Triangle>]) -> Self {
        let mut p = PackedTris::default();
        for (li, tl) in lists.iter().enumerate() {
            for t in tl {
                p.a.push(t[0]);
                p.e1.push(t[1] - t[0]);
                p.e2.push(t[2] - t[0]);
                p.face_idx.push(li);
            }
        }
        p
    }

    pub fn len(&self) -> usize {
        self.a.len()
    }

    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }

    fn subset(&self, rows: &[usize]) -> PackedTris {
        PackedTris {
            a: rows.iter().map(|&r| self.a[r]).collect(),
            e1: rows.iter().map(|&r| self.e1[r]).collect(),
            e2: rows.iter().map(|&r| self.e2[r]).collect(),
            face_idx: rows.iter().map(|&r| self.face_idx[r]).collect(),
        }
    }
}

/// One origin, many jittered rays: the origin-dependent terms are computed
/// once; each ray then costs a couple of vector ops.
struct ParityQuery<'a> {
    packed: &'a PackedTris,
    mask: Option<&'a [bool]>,
    tvec: Vec<DVec3>,
    q: Vec<DVec3>,
    e2q: Vec<f64>,
}

impl<'a> ParityQuery<'a> {
    fn new(origin: DVec3, packed: &'a PackedTris, mask: Option<&'a [bool]>) -> Self {
        let tvec: Vec<DVec3> = packed.a.iter().map(|&a| origin - a).collect();
        let q: Vec<DVec3> = tvec
            .iter()
            .zip(packed.e1.iter())
            .map(|(t, e1)| t.cross(*e1))
            .collect();
        let e2q: Vec<f64> = packed
            .e2
            .iter()
            .zip(q.iter())
            .map(|(e2, q)| e2.dot(*q))
            .collect();
        ParityQuery {
            packed,
            mask,
            tvec,
            q,
            e2q,
        }
    }

    fn crossings(&self, d: DVec3) -> usize {
        let mut count = 0;
        for i in 0..self.packed.len() {
            if let Some(m) = self.mask {
                if !m[i] {
                    continue;
                }
            }
            let p = d.cross(self.packed.e2[i]);
            let det = self.packed.e1[i].dot(p);
            if det.abs() < EPS {
                continue;
            }
            let inv = 1.0 / det;
            let u = self.tvec[i].dot(p) * inv;
            if !(-EPS..=1.0 + EPS).contains(&u) {
                continue;
            }
            let v = self.q[i].dot(d) * inv;
            if !(v >= -EPS && u + v <= 1.0 + EPS) {
                continue;
            }
            let t = self.e2q[i] * inv;
            if t > T_MIN {
                count += 1;
            }
        }
        count
    }
}

/// Whether `origin` sits *outside* the volume bounded by the triangles, by
/// crossing parity along jittered rays around `direction` (majority vote).
/// `None` on a tie. The engine's one volumetric primitive.
pub fn ray_parity_outside(
    origin: DVec3,
    direction: DVec3,
    packed: &PackedTris,
    mask: Option<&[bool]>,
    rng: &mut Mt19937,
) -> Option<bool> {
    let query = ParityQuery::new(origin, packed, mask);
    let n = direction.unit_or_zero();
    let (u, v) = basis(n);
    let mut outside = 0;
    let mut inside = 0;
    for r in 0..RAYS {
        let d = if r == 0 {
            n
        } else {
            let du = rng.uniform(-JITTER, JITTER);
            let dv = rng.uniform(-JITTER, JITTER);
            (n + u * du + v * dv).unit_or_zero()
        };
        if query.crossings(d) % 2 == 0 {
            outside += 1;
        } else {
            inside += 1;
        }
    }
    if outside == inside {
        return None;
    }
    Some(outside > inside)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideState {
    Outward,
    Inward,
    Interior,
}

/// Classify `face` against the volume bounded by the packed boundary
/// triangles (minus this face, via `mask`).
fn face_side_state(
    mesh: &Mesh,
    face: FaceId,
    packed: &PackedTris,
    mask: &[bool],
    rng: &mut Mt19937,
) -> Option<SideState> {
    let n = mesh.face_normal(face);
    if n.length() < 1e-9 {
        return None;
    }
    let probe = interior_probe(mesh, face);
    let ahead = ray_parity_outside(probe, n, packed, Some(mask), rng);
    if ahead != Some(false) {
        return if ahead == Some(true) {
            Some(SideState::Outward)
        } else {
            None
        };
    }
    let behind = ray_parity_outside(probe, -n, packed, Some(mask), rng);
    match behind {
        Some(true) => Some(SideState::Inward),
        Some(false) => Some(SideState::Interior),
        None => None,
    }
}

/// A point on the face's solid material: the centroid for a hole-free face
/// whose centroid lies on it, else the largest triangle's centroid.
pub fn interior_probe(mesh: &Mesh, face: FaceId) -> DVec3 {
    if mesh.face(face).holes.is_empty() {
        let c = mesh.face_centroid(face);
        if on_face(mesh, face, c) {
            return c;
        }
    }
    let tris = mesh.face_triangulate(face, None);
    if tris.is_empty() {
        return mesh.face_centroid(face);
    }
    let best = tris
        .iter()
        .max_by(|a, b| {
            let aa = (a[1] - a[0]).cross(a[2] - a[0]).length_squared();
            let bb = (b[1] - b[0]).cross(b[2] - b[0]).length_squared();
            aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .unwrap_or(tris[0]);
    (best[0] + best[1] + best[2]) / 3.0
}

fn on_face(mesh: &Mesh, face: FaceId, p: DVec3) -> bool {
    let n = mesh.face_normal(face);
    if n.length() < 1e-12 {
        return true;
    }
    let (u, v) = plane_basis(n.unit_or_zero());
    let lp: Vec<(f64, f64)> = mesh
        .face_positions(face)
        .iter()
        .map(|&q| ((q - p).dot(u), (q - p).dot(v)))
        .collect();
    point_in_polygon((0.0, 0.0), &lp)
}

fn all_coplanar(mesh: &Mesh, faces: &[FaceId]) -> bool {
    let first = faces[0];
    let n = mesh.face_normal(first);
    let o = mesh.face_centroid(first);
    for &f in faces {
        let fnm = mesh.face_normal(f);
        if fnm.dot(n).abs() < 0.999 {
            return false;
        }
        if (mesh.face_centroid(f) - o).dot(n).abs() > 1e-4 {
            return false;
        }
    }
    true
}

/// Whether the mesh has no boundary: every edge borders at least two faces.
pub fn is_closed(mesh: &Mesh) -> bool {
    if mesh.faces.is_empty() {
        return false;
    }
    mesh.edges.iter().all(|(_, e)| e.faces.len() >= 2)
}

/// Signed volume of the mesh as currently wound; interior partitions are
/// skipped.
pub fn signed_volume(mesh: &Mesh) -> f64 {
    let mut total = 0.0;
    for (fid, f) in mesh.faces.iter() {
        if f.interior {
            continue;
        }
        for [a, b, c] in mesh.face_triangulate(fid, None) {
            total += a.dot(b.cross(c)) / 6.0;
        }
    }
    total
}

/// Face → edge-connected component label (union-find through shared edges).
fn face_components(mesh: &Mesh, faces: &[FaceId]) -> HashMap<FaceId, usize> {
    let idx: HashMap<FaceId, usize> = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (_, e) in mesh.edges.iter() {
        let mut it = e.faces.iter();
        let first = loop {
            match it.next() {
                Some(f) => {
                    if let Some(&i) = idx.get(f) {
                        break Some(i);
                    }
                }
                None => break None,
            }
        };
        let Some(first) = first else { continue };
        for f in it {
            if let Some(&j) = idx.get(f) {
                let ra = find(&mut parent, first);
                let rb = find(&mut parent, j);
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut out = HashMap::new();
    for (i, &f) in faces.iter().enumerate() {
        out.insert(f, find(&mut parent, i));
    }
    out
}

/// Flip the faces of a closed solid so every boundary normal points outward,
/// and mark interior partitions (`face.interior`). Returns the faces flipped.
///
/// `only` restricts the pass to the edge-connected components those faces
/// belong to; every other face keeps its winding and its `interior` mark.
pub fn orient_outward(mesh: &mut Mesh, seed: u64, only: Option<&[FaceId]>) -> Vec<FaceId> {
    let faces = mesh.face_ids();
    if faces.is_empty() {
        return Vec::new();
    }
    if all_coplanar(mesh, &faces) {
        for &f in &faces {
            mesh.faces[f].interior = false;
        }
        return Vec::new();
    }
    let all_tris: Vec<Vec<Triangle>> = faces
        .iter()
        .map(|&f| mesh.face_triangulate(f, None))
        .collect();
    let fidx: HashMap<FaceId, usize> = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let packed = PackedTris::from_lists(&all_tris);
    let comp_of = face_components(mesh, &faces);
    let comp_idx: Vec<usize> = faces.iter().map(|f| comp_of[f]).collect();
    let mut comp_rows: HashMap<usize, Vec<usize>> = HashMap::new();
    for (r, &fi) in packed.face_idx.iter().enumerate() {
        comp_rows.entry(comp_idx[fi]).or_default().push(r);
    }
    for &c in &comp_idx {
        comp_rows.entry(c).or_default();
    }
    let sub: HashMap<usize, PackedTris> = comp_rows
        .iter()
        .map(|(&c, rows)| (c, packed.subset(rows)))
        .collect();

    let scope: Vec<FaceId> = match only {
        None => faces.clone(),
        Some(only) => {
            let live: HashSet<usize> = only
                .iter()
                .filter_map(|f| comp_of.get(f).copied())
                .collect();
            if live.is_empty() {
                return Vec::new();
            }
            faces
                .iter()
                .copied()
                .filter(|f| live.contains(&comp_of[f]))
                .collect()
        }
    };

    let base_mask = |boundary: &HashSet<FaceId>, c: usize, exclude: usize| -> Vec<bool> {
        let sp = &sub[&c];
        sp.face_idx
            .iter()
            .map(|&fi| fi != exclude && boundary.contains(&faces[fi]))
            .collect()
    };

    let mut boundary: HashSet<FaceId> = faces.iter().copied().collect();
    for _ in 0..4 {
        let mut rng = Mt19937::new(seed);
        let mut interior_now: HashSet<FaceId> = HashSet::new();
        for &f in &scope {
            let c = comp_of[&f];
            let mask = base_mask(&boundary, c, fidx[&f]);
            let state = face_side_state(mesh, f, &sub[&c], &mask, &mut rng);
            if state == Some(SideState::Interior) {
                interior_now.insert(f);
            }
        }
        let current_interior: HashSet<FaceId> = scope
            .iter()
            .copied()
            .filter(|f| !boundary.contains(f))
            .collect();
        if interior_now == current_interior {
            break;
        }
        boundary = faces
            .iter()
            .copied()
            .filter(|f| !interior_now.contains(f))
            .collect();
    }
    for &f in &scope {
        mesh.faces[f].interior = !boundary.contains(&f);
    }
    if !is_closed(mesh) {
        return Vec::new();
    }
    let mut rng = Mt19937::new(seed);
    let mut to_flip = Vec::new();
    for &f in &scope {
        if !boundary.contains(&f) {
            continue;
        }
        let c = comp_of[&f];
        let mask = base_mask(&boundary, c, fidx[&f]);
        if face_side_state(mesh, f, &sub[&c], &mask, &mut rng) == Some(SideState::Inward) {
            to_flip.push(f);
        }
    }
    for &f in &to_flip {
        mesh.turn_face_over(f);
    }
    to_flip
}

/// The default seed (the tolerances and fuzz regressions were hardened
/// around it).
pub const DEFAULT_SEED: u64 = 12345;
