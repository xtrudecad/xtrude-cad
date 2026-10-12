// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Planar arrangement: rebuild the minimal faces of a plane from its edges.
//!
//! Given a tangle of segments lying in one plane, recompute the *minimal
//! bounded faces* (classic DCEL face extraction):
//!
//! 1. Project the segments to the plane's 2D coordinates.
//! 2. Split every segment at its intersections / overlaps with the others,
//!    snapping endpoints to a quantised grid so coincident points become one
//!    vertex.
//! 3. Prune dangling (degree-1) chains — they border no face.
//! 4. At each vertex, sort the outgoing half-edges by angle; trace each face
//!    by always taking the *next edge clockwise*, which yields bounded faces
//!    wound CCW and the unbounded face wound CW.
//! 5. Keep the CCW cycles as faces; nest each CW cycle (a hole) inside the
//!    smallest CCW face that contains it; drop the one CW cycle that contains
//!    everything.

use indexmap::{IndexMap, IndexSet};

use crate::math::{DVec3, VecExt, MIN_FACE_AREA};

/// Position quantisation (~0.1 mm at metre scale).
const TOL: f64 = 1e-4;
const EPS: f64 = 1e-7;
/// Perpendicular slack for "point lies on segment".
const ON_SEG: f64 = 1e-4;

pub type P2 = (f64, f64);
type K2 = (i64, i64);

fn key2(x: f64, y: f64) -> K2 {
    (
        crate::math::round_even_int(x / TOL),
        crate::math::round_even_int(y / TOL),
    )
}

/// Two orthonormal in-plane axes for `normal` (stable choice of reference).
pub fn plane_basis(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.unit_or_zero();
    let mut reference = DVec3::X;
    if reference.dot(n).abs() > 0.9 {
        reference = DVec3::Y;
    }
    let u = (reference - n * reference.dot(n)).unit_or_zero();
    let v = n.cross(u).unit_or_zero();
    (u, v)
}

/// Parameter `t` of `p` on segment `a`–`b` (clamped to [0,1]) if it lies on
/// it within `ON_SEG`, else `None`.
fn on_seg(p: P2, a: P2, b: P2) -> Option<f64> {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let l2 = abx * abx + aby * aby;
    if l2 < 1e-12 {
        return None;
    }
    let t = ((p.0 - a.0) * abx + (p.1 - a.1) * aby) / l2;
    let (px, py) = (a.0 + t * abx, a.1 + t * aby);
    if (p.0 - px).powi(2) + (p.1 - py).powi(2) > ON_SEG * ON_SEG {
        return None;
    }
    if !(-EPS..=1.0 + EPS).contains(&t) {
        return None;
    }
    Some(t.clamp(0.0, 1.0))
}

/// All meeting points of segments `a`–`b` and `c`–`d`: the proper crossing,
/// or the overlap endpoints when collinear.
fn intersections(a: P2, b: P2, c: P2, d: P2) -> Vec<P2> {
    let (rx, ry) = (b.0 - a.0, b.1 - a.1);
    let (sx, sy) = (d.0 - c.0, d.1 - c.1);
    let rxs = rx * sy - ry * sx;
    let (qpx, qpy) = (c.0 - a.0, c.1 - a.1);
    let mut pts = Vec::new();
    if rxs.abs() > 1e-9 {
        let t = (qpx * sy - qpy * sx) / rxs;
        let u = (qpx * ry - qpy * rx) / rxs;
        if (-EPS..=1.0 + EPS).contains(&t) && (-EPS..=1.0 + EPS).contains(&u) {
            pts.push((a.0 + t * rx, a.1 + t * ry));
        }
    } else if (qpx * ry - qpy * rx).abs() < 1e-9 {
        for p in [c, d] {
            if on_seg(p, a, b).is_some() {
                pts.push(p);
            }
        }
        for p in [a, b] {
            if on_seg(p, c, d).is_some() {
                pts.push(p);
            }
        }
    }
    pts
}

pub fn signed_area(lp: &[P2]) -> f64 {
    let n = lp.len();
    let mut s = 0.0;
    for i in 0..n {
        let (x0, y0) = lp[i];
        let (x1, y1) = lp[(i + 1) % n];
        s += x0 * y1 - x1 * y0;
    }
    s * 0.5
}

/// Ray-cast even-odd test.
pub fn point_in_polygon(p: P2, poly: &[P2]) -> bool {
    let (x, y) = p;
    let mut inside = false;
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi + 1e-30) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

type Graph = (IndexMap<K2, P2>, IndexMap<K2, IndexSet<K2>>);

fn build_graph(segments: &[(P2, P2)]) -> Graph {
    let n = segments.len();
    let mut split_pts: Vec<Vec<P2>> = segments.iter().map(|&(a, b)| vec![a, b]).collect();
    for i in 0..n {
        let (a, b) = segments[i];
        for (j, &(c, d)) in segments.iter().enumerate() {
            if i == j {
                continue;
            }
            for p in intersections(a, b, c, d) {
                if on_seg(p, a, b).is_some() {
                    split_pts[i].push(p);
                }
            }
        }
    }
    let mut coords: IndexMap<K2, P2> = IndexMap::new();
    let mut adj: IndexMap<K2, IndexSet<K2>> = IndexMap::new();
    let vert = |p: P2, coords: &mut IndexMap<K2, P2>, adj: &mut IndexMap<K2, IndexSet<K2>>| {
        let k = key2(p.0, p.1);
        coords.entry(k).or_insert(p);
        adj.entry(k).or_default();
        k
    };
    for i in 0..n {
        let (a, b) = segments[i];
        let pts = &mut split_pts[i];
        pts.sort_by(|p, q| {
            let tp = on_seg(*p, a, b).unwrap_or(0.0);
            let tq = on_seg(*q, a, b).unwrap_or(0.0);
            tp.partial_cmp(&tq).unwrap_or(std::cmp::Ordering::Equal)
        });
        for k in 0..pts.len().saturating_sub(1) {
            let ka = vert(pts[k], &mut coords, &mut adj);
            let kb = vert(pts[k + 1], &mut coords, &mut adj);
            if ka != kb {
                adj[&ka].insert(kb);
                adj[&kb].insert(ka);
            }
        }
    }
    (coords, adj)
}

fn prune_dangling(adj: &mut IndexMap<K2, IndexSet<K2>>) {
    let mut changed = true;
    while changed {
        changed = false;
        let keys: Vec<K2> = adj.keys().copied().collect();
        for k in keys {
            let Some(nbrs) = adj.get(&k) else { continue };
            if nbrs.len() == 1 {
                let other = *nbrs.first().expect("one neighbour");
                if let Some(o) = adj.get_mut(&other) {
                    o.shift_remove(&k);
                }
                adj.shift_remove(&k);
                changed = true;
            } else if nbrs.is_empty() {
                adj.shift_remove(&k);
                changed = true;
            }
        }
    }
}

fn trace_faces(coords: &IndexMap<K2, P2>, adj: &IndexMap<K2, IndexSet<K2>>) -> Vec<Vec<K2>> {
    let mut order: IndexMap<K2, Vec<K2>> = IndexMap::new();
    for (k, nbrs) in adj {
        let (kx, ky) = coords[k];
        let mut ring: Vec<K2> = nbrs.iter().copied().collect();
        ring.sort_by(|m1, m2| {
            let a1 = (coords[m1].1 - ky).atan2(coords[m1].0 - kx);
            let a2 = (coords[m2].1 - ky).atan2(coords[m2].0 - kx);
            a1.partial_cmp(&a2).unwrap_or(std::cmp::Ordering::Equal)
        });
        order.insert(*k, ring);
    }
    let next_he = |u: K2, v: K2| -> K2 {
        let ring = &order[&v];
        let idx = ring.iter().position(|&x| x == u).unwrap_or(0);
        // previous in CCW-sorted == clockwise next
        ring[(idx + ring.len() - 1) % ring.len()]
    };
    let mut visited: std::collections::HashSet<(K2, K2)> = std::collections::HashSet::new();
    let mut loops = Vec::new();
    for (&u, nbrs) in adj {
        for &v in nbrs {
            if visited.contains(&(u, v)) {
                continue;
            }
            let mut lp = Vec::new();
            let (mut cu, mut cv) = (u, v);
            while !visited.contains(&(cu, cv)) {
                visited.insert((cu, cv));
                lp.push(cu);
                let nx = next_he(cu, cv);
                cu = cv;
                cv = nx;
                if lp.len() > 100_000 {
                    break; // pathological guard
                }
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    loops
}

pub type FaceXY = (Vec<P2>, Vec<Vec<P2>>);

fn classify(loops: &[Vec<K2>], coords: &IndexMap<K2, P2>) -> Vec<FaceXY> {
    let mut faces: Vec<(Vec<P2>, f64)> = Vec::new();
    let mut holes: Vec<(Vec<P2>, f64)> = Vec::new();
    for lp in loops {
        let xy: Vec<P2> = lp.iter().map(|k| coords[k]).collect();
        let area = signed_area(&xy);
        if area > MIN_FACE_AREA {
            faces.push((xy, area));
        } else if area < -MIN_FACE_AREA {
            holes.push((xy, -area));
        }
    }
    if faces.is_empty() {
        return Vec::new();
    }
    if !holes.is_empty() {
        holes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        holes.remove(0); // discard the all-enclosing outer
    }
    faces.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut face_holes: Vec<Vec<Vec<P2>>> = vec![Vec::new(); faces.len()];
    for (hxy, harea) in holes {
        let hp = interior_point(&hxy);
        for (fi, (fxy, farea)) in faces.iter().enumerate() {
            if *farea > harea + MIN_FACE_AREA && point_in_polygon(hp, fxy) {
                face_holes[fi].push(hxy.clone());
                break;
            }
        }
    }
    faces
        .into_iter()
        .zip(face_holes)
        .map(|((fxy, _), hls)| (fxy, hls))
        .collect()
}

struct Arranged {
    coords: IndexMap<K2, P2>,
    full_adj: IndexMap<K2, IndexSet<K2>>,
    faces_xy: Vec<FaceXY>,
    origin: DVec3,
    u: DVec3,
    v: DVec3,
}

impl Arranged {
    fn to3d(&self, xy: P2) -> DVec3 {
        self.origin + self.u * xy.0 + self.v * xy.1
    }
}

fn arrange(segments_3d: &[(DVec3, DVec3)], origin: DVec3, normal: DVec3) -> Arranged {
    let (u, v) = plane_basis(normal);
    let to2d = |p: DVec3| -> P2 {
        let d = p - origin;
        (d.dot(u), d.dot(v))
    };
    let mut segs = Vec::new();
    for &(a, b) in segments_3d {
        let (pa, pb) = (to2d(a), to2d(b));
        if (pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2) > TOL * TOL {
            segs.push((pa, pb));
        }
    }
    if segs.is_empty() {
        return Arranged {
            coords: IndexMap::new(),
            full_adj: IndexMap::new(),
            faces_xy: Vec::new(),
            origin,
            u,
            v,
        };
    }
    let (coords, full_adj) = build_graph(&segs);
    let mut adj = full_adj.clone();
    prune_dangling(&mut adj);
    let loops = trace_faces(&coords, &adj);
    let faces_xy = classify(&loops, &coords);
    Arranged {
        coords,
        full_adj,
        faces_xy,
        origin,
        u,
        v,
    }
}

/// A face as 3D loops: `(outer, holes)`.
pub type Face3 = (Vec<DVec3>, Vec<Vec<DVec3>>);

/// Rebuild minimal faces from coplanar `segments_3d`.
pub fn planar_arrangement(
    segments_3d: &[(DVec3, DVec3)],
    origin: DVec3,
    normal: DVec3,
) -> Vec<Face3> {
    let ar = arrange(segments_3d, origin, normal);
    ar.faces_xy
        .iter()
        .map(|(outer, holes)| {
            (
                outer.iter().map(|&p| ar.to3d(p)).collect(),
                holes
                    .iter()
                    .map(|h| h.iter().map(|&p| ar.to3d(p)).collect())
                    .collect(),
            )
        })
        .collect()
}

/// Full planar rebuild: `(edges_3d, faces_3d)` — the complete split edge set
/// (nothing lost, spurs included) and the faces.
pub fn planar_rebuild(
    segments_3d: &[(DVec3, DVec3)],
    origin: DVec3,
    normal: DVec3,
) -> (Vec<(DVec3, DVec3)>, Vec<Face3>) {
    let ar = arrange(segments_3d, origin, normal);
    let mut edges = Vec::new();
    let mut seen: std::collections::HashSet<(K2, K2)> = std::collections::HashSet::new();
    for (k, nbrs) in &ar.full_adj {
        for m in nbrs {
            let key = if k <= m { (*k, *m) } else { (*m, *k) };
            if !seen.insert(key) {
                continue;
            }
            edges.push((ar.to3d(ar.coords[k]), ar.to3d(ar.coords[m])));
        }
    }
    let faces = ar
        .faces_xy
        .iter()
        .map(|(outer, holes)| {
            (
                outer.iter().map(|&p| ar.to3d(p)).collect(),
                holes
                    .iter()
                    .map(|h| h.iter().map(|&p| ar.to3d(p)).collect())
                    .collect(),
            )
        })
        .collect();
    (edges, faces)
}

/// `(origin, normal)` of the plane through `points` if they are all
/// coplanar, else `None`. Collinear or degenerate input returns a horizontal
/// plane (the arrangement then yields no face).
pub fn coplanar_plane(points: &[DVec3], tol: f64) -> Option<(DVec3, DVec3)> {
    if points.len() < 3 {
        return Some((points.first().copied().unwrap_or(DVec3::ZERO), DVec3::Z));
    }
    let o = points[0];
    let Some(d1) = points[1..]
        .iter()
        .find(|p| (**p - o).length() > 1e-6)
        .map(|p| (*p - o).unit_or_zero())
    else {
        return Some((o, DVec3::Z));
    };
    let Some(n) = points[1..]
        .iter()
        .map(|p| d1.cross(*p - o))
        .find(|c| c.length() > 1e-6)
        .map(|c| c.unit_or_zero())
    else {
        return Some((o, DVec3::Z));
    };
    for p in points {
        if n.dot(*p - o).abs() > tol {
            return None;
        }
    }
    Some((o, n))
}

/// A point safely inside the polygon `lp`, good enough for nesting.
pub fn interior_point(lp: &[P2]) -> P2 {
    let n = lp.len() as f64;
    let cx = lp.iter().map(|p| p.0).sum::<f64>() / n;
    let cy = lp.iter().map(|p| p.1).sum::<f64>() / n;
    if point_in_polygon((cx, cy), lp) {
        return (cx, cy);
    }
    let (ax, ay) = lp[0];
    let (bx, by) = lp[1 % lp.len()];
    let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
    let p = (mx * 0.999 + cx * 0.001, my * 0.999 + cy * 0.001);
    if point_in_polygon(p, lp) {
        return p;
    }
    scan_interior_point(lp, &[]).unwrap_or(p)
}

/// A point well inside `outer` and outside every hole, or `None`: the middle
/// of the widest span of a horizontal scan line between two vertex heights.
pub fn scan_interior_point(outer: &[P2], holes: &[Vec<P2>]) -> Option<P2> {
    let loops: Vec<&[P2]> = std::iter::once(outer)
        .chain(holes.iter().map(|h| h.as_slice()))
        .collect();
    let mut ys: Vec<f64> = loops.iter().flat_map(|lp| lp.iter().map(|p| p.1)).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup();
    let mut best: Option<(f64, P2)> = None;
    for w2 in ys.windows(2) {
        let (y0, y1) = (w2[0], w2[1]);
        if y1 - y0 < 1e-9 {
            continue;
        }
        let y = (y0 + y1) / 2.0;
        let mut xs = Vec::new();
        for lp in &loops {
            let n = lp.len();
            for i in 0..n {
                let (ax, ay) = lp[i];
                let (bx, by) = lp[(i + 1) % n];
                if (ay > y) != (by > y) {
                    xs.push(ax + (y - ay) * (bx - ax) / (by - ay));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            let w = (b - a).min(y1 - y0);
            if best.is_none_or(|(bw, _)| w > bw) {
                best = Some((w, ((a + b) / 2.0, y)));
            }
        }
    }
    best.map(|(_, p)| p)
}
