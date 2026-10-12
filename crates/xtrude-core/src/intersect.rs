// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Crossing edges: new edges wherever faces cross.
//!
//! Two planar faces cross along the line common to their two planes; that
//! line is clipped by each polygon (outer ring and holes, even-odd) and what
//! both keep is the new edge. Coplanar faces do not cross.

use crate::math::{newell, DMat4, DVec3, MatExt, VecExt};
use crate::mesh::{FaceId, Mesh};
use crate::triangulate::plane_axes;

/// Below this a crossing is a point, not an edge.
const MIN_LEN: f64 = 1e-5;

/// A face in world space, ready for crossing tests.
#[derive(Clone, Debug)]
pub struct Poly {
    pub rings: Vec<Vec<DVec3>>,
    pub n: DVec3,
    pub d: f64,
    pub lo: DVec3,
    pub hi: DVec3,
    u: DVec3,
    v: DVec3,
    rings2d: Vec<Vec<(f64, f64)>>,
}

impl Poly {
    pub fn new(rings: Vec<Vec<DVec3>>, normal: DVec3) -> Self {
        let n = normal.unit_or_zero();
        let d = n.dot(rings[0][0]);
        let lo = rings[0]
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
        let hi = rings[0]
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
        let (u, v) = plane_axes(n);
        let rings2d = rings
            .iter()
            .map(|r| r.iter().map(|p| (p.dot(u), p.dot(v))).collect())
            .collect();
        Poly {
            rings,
            n,
            d,
            lo,
            hi,
            u,
            v,
            rings2d,
        }
    }
}

/// `face` of `mesh` as a world-space polygon (through `xform` for a group).
pub fn world_poly(mesh: &Mesh, face: FaceId, xform: Option<&DMat4>) -> Option<Poly> {
    let w = |p: DVec3| xform.map_or(p, |m| m.project_point(p));
    let outer: Vec<DVec3> = mesh.face_positions(face).into_iter().map(w).collect();
    if outer.len() < 3 {
        return None;
    }
    let n = newell(&outer);
    if n.length() < 1e-12 {
        return None;
    }
    let mut rings = vec![outer];
    rings.extend(
        mesh.face_hole_positions(face)
            .into_iter()
            .map(|h| h.into_iter().map(w).collect()),
    );
    Some(Poly::new(rings, n))
}

fn boxes_touch(a: &Poly, b: &Poly) -> bool {
    let eps = 1e-6;
    (0..3).all(|i| a.lo[i] <= b.hi[i] + eps && b.lo[i] <= a.hi[i] + eps)
}

fn intervals(poly: &Poly, origin: DVec3, direction: DVec3) -> Vec<(f64, f64)> {
    let (ox, oy) = (origin.dot(poly.u), origin.dot(poly.v));
    let (dx, dy) = (direction.dot(poly.u), direction.dot(poly.v));
    let mut ts = Vec::new();
    for ring in &poly.rings2d {
        let n = ring.len();
        for i in 0..n {
            let (ax, ay) = ring[i];
            let (bx, by) = ring[(i + 1) % n];
            let sa = (ax - ox) * dy - (ay - oy) * dx;
            let sb = (bx - ox) * dy - (by - oy) * dx;
            if (sa >= 0.0) == (sb >= 0.0) {
                continue;
            }
            let k = sa / (sa - sb);
            let (px, py) = (ax + (bx - ax) * k, ay + (by - ay) * k);
            let den = dx * dx + dy * dy;
            ts.push(((px - ox) * dx + (py - oy) * dy) / den);
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ts.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

fn overlap(xs: &[(f64, f64)], ys: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let (mut out, mut i, mut j) = (Vec::new(), 0, 0);
    while i < xs.len() && j < ys.len() {
        let lo = xs[i].0.max(ys[j].0);
        let hi = xs[i].1.min(ys[j].1);
        if hi > lo {
            out.push((lo, hi));
        }
        if xs[i].1 < ys[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// The segments where faces `a` and `b` cross.
pub fn cross(a: &Poly, b: &Poly) -> Vec<(DVec3, DVec3)> {
    let direction = a.n.cross(b.n);
    if direction.length() < 1e-9 {
        return Vec::new();
    }
    let direction = direction.unit_or_zero();
    let c = a.n.dot(b.n);
    let den = 1.0 - c * c;
    let k1 = (a.d - b.d * c) / den;
    let k2 = (b.d - a.d * c) / den;
    let origin = a.n * k1 + b.n * k2;
    overlap(
        &intervals(a, origin, direction),
        &intervals(b, origin, direction),
    )
    .into_iter()
    .map(|(t0, t1)| (origin + direction * t0, origin + direction * t1))
    .filter(|(p, q)| (*q - *p).length() > MIN_LEN)
    .collect()
}

/// Every crossing between a face of `a` and one of `b` (`same`: one list,
/// each pair once).
pub fn intersect(a: &[Poly], b: &[Poly], same: bool) -> Vec<(DVec3, DVec3)> {
    let mut out = Vec::new();
    for (i, pa) in a.iter().enumerate() {
        let start = if same { i + 1 } else { 0 };
        for pb in b.iter().skip(start) {
            if !boxes_touch(pa, pb) {
                continue;
            }
            out.extend(cross(pa, pb));
        }
    }
    dedupe(out)
}

fn dedupe(segs: Vec<(DVec3, DVec3)>) -> Vec<(DVec3, DVec3)> {
    let tol = 1e-6;
    let k = |p: DVec3| {
        (
            crate::math::round_even_int(p.x / tol),
            crate::math::round_even_int(p.y / tol),
            crate::math::round_even_int(p.z / tol),
        )
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (p, q) in segs {
        let (k1, k2) = (k(p), k(q));
        let key = if k1 <= k2 { (k1, k2) } else { (k2, k1) };
        if seen.insert(key) {
            out.push((p, q));
        }
    }
    out
}
