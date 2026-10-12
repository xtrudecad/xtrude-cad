// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Sweep: extrude a profile face along an edge path.
//!
//! The discrete prism-miter construction: at every path vertex a joint
//! plane is placed (normal = bisector of the incoming and outgoing
//! directions; at open ends, the segment direction). The profile's ring at
//! each station is the previous ring slid parallel to the incoming segment
//! onto the joint plane — the exact miter. A closed circular path whose axis
//! lies in the profile's plane is a lathe and turns the profile
//! about the axis instead.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::geometry::PolyFace;
use crate::math::{DVec3, VecExt};
use crate::mesh::{EdgeId, FaceId, Mesh, VertexId};
use crate::orient::{orient_outward, DEFAULT_SEED};
use crate::stitch::run_stitch;

/// Dihedral cosine above which a sweep seam reads as a curve facet.
pub const CURVE_FACET_COS: f64 = 0.85;

/// Chain the selected edges into an ordered polyline: `(points, closed)`,
/// or `None` when they do not form one simple chain.
pub fn order_path_edges(mesh: &Mesh, edges: &[EdgeId]) -> Option<(Vec<DVec3>, bool)> {
    if edges.is_empty() {
        return None;
    }
    let mut adj: indexmap::IndexMap<VertexId, Vec<VertexId>> = indexmap::IndexMap::new();
    for &e in edges {
        let ed = mesh.edge(e);
        adj.entry(ed.v0).or_default().push(ed.v1);
        adj.entry(ed.v1).or_default().push(ed.v0);
    }
    if adj.values().any(|n| n.len() > 2) {
        return None;
    }
    let ends: Vec<VertexId> = adj
        .iter()
        .filter(|(_, n)| n.len() == 1)
        .map(|(v, _)| *v)
        .collect();
    if !(ends.is_empty() || ends.len() == 2) {
        return None;
    }
    let closed = ends.is_empty();
    let start = ends.first().copied().unwrap_or(mesh.edge(edges[0]).v0);
    let mut pts = vec![mesh.pos(start)];
    let (mut prev, mut cur) = (None, start);
    for _ in 0..edges.len() {
        let nxt = adj[&cur].iter().copied().find(|&v| Some(v) != prev)?;
        pts.push(mesh.pos(nxt));
        prev = Some(cur);
        cur = nxt;
    }
    if closed {
        if cur != start {
            return None;
        }
        pts.pop();
    } else if pts.len() != edges.len() + 1 {
        return None;
    }
    Some((pts, closed))
}

fn project_ring(
    points: &[DVec3],
    direction: DVec3,
    plane_pt: DVec3,
    plane_n: DVec3,
) -> Option<Vec<DVec3>> {
    let denom = direction.dot(plane_n);
    if denom.abs() < 1e-9 {
        return None;
    }
    Some(
        points
            .iter()
            .map(|&p| p + direction * ((plane_pt - p).dot(plane_n) / denom))
            .collect(),
    )
}

/// A wall quad without its repeated corners, or `None` when fewer than
/// three distinct corners remain.
pub fn wall(quad: &[DVec3]) -> Option<Vec<DVec3>> {
    let mut out: Vec<DVec3> = Vec::new();
    for &p in quad {
        if out.last().is_none_or(|l| (p - *l).length() >= 1e-9) {
            out.push(p);
        }
    }
    if out.len() > 1 && (out[0] - out[out.len() - 1]).length() < 1e-9 {
        out.pop();
    }
    (out.len() >= 3).then_some(out)
}

type P3 = (f64, f64, f64);

fn clip_half(lp: &[P3], keep_tol: f64) -> Option<Vec<P3>> {
    let mut out: Vec<P3> = Vec::new();
    let m = lp.len();
    for i in 0..m {
        let (cur, nxt) = (lp[i], lp[(i + 1) % m]);
        let cin = cur.1 >= -keep_tol;
        let nin = nxt.1 >= -keep_tol;
        if cin {
            out.push(if cur.1 > keep_tol {
                (cur.0, cur.1.max(0.0), cur.2)
            } else {
                (cur.0, 0.0, 0.0)
            });
        }
        if cin != nin {
            let t = cur.1 / (cur.1 - nxt.1);
            out.push((cur.0 + (nxt.0 - cur.0) * t, 0.0, 0.0));
        }
    }
    let differs = |a: P3, b: P3| {
        (a.0 - b.0).abs() > 1e-12 || (a.1 - b.1).abs() > 1e-12 || (a.2 - b.2).abs() > 1e-12
    };
    let mut clean: Vec<P3> = Vec::new();
    for q in out {
        if clean.last().is_none_or(|l| differs(q, *l)) {
            clean.push(q);
        }
    }
    while clean.len() > 1 && !differs(clean[0], clean[clean.len() - 1]) {
        clean.pop();
    }
    (clean.len() >= 3).then_some(clean)
}

/// Rings, unit span directions and span count of a sweep.
pub type Rings = (Vec<Vec<Vec<DVec3>>>, Vec<DVec3>, usize);

fn revolution_rings(face: &PolyFace, path: &[DVec3]) -> Option<Rings> {
    let n = path.len();
    if n < 3 {
        return None;
    }
    let c = path.iter().copied().sum::<DVec3>() / n as f64;
    let ax = crate::math::newell(path);
    if ax.length() < 1e-12 {
        return None;
    }
    let ax = ax / ax.length();
    let rel: Vec<DVec3> = path.iter().map(|&p| p - c).collect();
    let radii: Vec<f64> = rel.iter().map(|r| r.length()).collect();
    let rmean = radii.iter().sum::<f64>() / n as f64;
    if rmean < 1e-9 {
        return None;
    }
    let tol = 1e-4f64.max(1e-3 * rmean);
    if rel.iter().any(|r| r.dot(ax).abs() > tol) {
        return None;
    }
    let (rmin, rmax) = radii
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &r| {
            (a.min(r), b.max(r))
        });
    if rmax - rmin > tol + 1e-2 * rmean {
        return None;
    }
    let fnm = face.normal();
    if fnm.length() < 1e-12 {
        return None;
    }
    let fnm = fnm / fnm.length();
    if fnm.dot(ax).abs() > 1e-3 {
        return None;
    }
    if (face.vertices[0] - c).dot(fnm).abs() > tol {
        return None;
    }
    let mut radial = ax.cross(fnm);
    radial /= radial.length();
    if (face.centroid() - c).dot(radial) < 0.0 {
        radial = -radial;
    }
    let side = ax.cross(radial);
    let local = |p: DVec3| -> P3 {
        let r = p - c;
        (r.dot(ax), r.dot(radial), r.dot(side))
    };
    let size = face
        .vertices
        .iter()
        .map(|&v| {
            let q = local(v);
            q.0.abs().max(q.1.abs()).max(q.2.abs())
        })
        .fold(0.0, f64::max);
    let size = if size == 0.0 { 1.0 } else { size };
    let keep_tol = 1e-7f64.max(1e-6 * size);
    let mut outer: Vec<P3> = face.vertices.iter().map(|&v| local(v)).collect();
    if outer.iter().map(|q| q.1).fold(f64::INFINITY, f64::min) < -keep_tol {
        outer = clip_half(&outer, keep_tol)?;
    } else {
        outer = outer
            .into_iter()
            .map(|q| {
                if q.1.abs() > keep_tol {
                    (q.0, q.1, 0.0)
                } else {
                    (q.0, 0.0, 0.0)
                }
            })
            .collect();
    }
    let mut holes: Vec<Vec<P3>> = Vec::new();
    for h in &face.holes {
        let hl: Vec<P3> = h.iter().map(|&v| local(v)).collect();
        let xs: Vec<f64> = hl.iter().map(|q| q.1).collect();
        let (xmin, xmax) = xs
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                (a.min(x), b.max(x))
            });
        if xmin >= -keep_tol {
            holes.push(hl);
        } else if xmax > keep_tol {
            return None;
        }
    }
    let mut loops = vec![outer];
    loops.extend(holes);
    let mut rings = Vec::new();
    for r in &rel {
        let (ca, sa) = (r.dot(radial), r.dot(side));
        let norm = ca.hypot(sa);
        let norm = if norm == 0.0 { 1.0 } else { norm };
        let (ca, sa) = (ca / norm, sa / norm);
        let ring: Vec<Vec<DVec3>> = loops
            .iter()
            .map(|lp| {
                lp.iter()
                    .map(|&(h, x, y)| {
                        c + ax * h + (radial * ca + side * sa) * x + (side * ca - radial * sa) * y
                    })
                    .collect()
            })
            .collect();
        rings.push(ring);
    }
    let mut dirs = Vec::new();
    for i in 0..n {
        let d = path[(i + 1) % n] - path[i];
        if d.length() < 1e-9 {
            return None;
        }
        dirs.push(d.unit_or_zero());
    }
    Some((rings, dirs, n))
}

/// The profile's rings (outer + holes) at every path station.
pub fn sweep_rings(face: &PolyFace, path: &[DVec3], closed: bool) -> Option<Rings> {
    if closed {
        if let Some(r) = revolution_rings(face, path) {
            return Some(r);
        }
    }
    let n = path.len();
    let spans = if closed { n } else { n.saturating_sub(1) };
    if spans < 1 {
        return None;
    }
    let mut dirs = Vec::new();
    for i in 0..spans {
        let d = path[(i + 1) % n] - path[i];
        if d.length() < 1e-9 {
            return None;
        }
        dirs.push(d.unit_or_zero());
    }
    let joint_normal = |i: usize| -> Option<DVec3> {
        let (a, b) = if closed {
            (dirs[(i + spans - 1) % spans], dirs[i % spans])
        } else if i == 0 {
            return Some(dirs[0]);
        } else if i >= spans {
            return Some(dirs[spans - 1]);
        } else {
            (dirs[i - 1], dirs[i])
        };
        let s = a + b;
        (s.length() >= 1e-9).then(|| s.unit_or_zero())
    };
    let mut loops = vec![face.vertices.clone()];
    loops.extend(face.holes.iter().cloned());
    let mut rings: Vec<Vec<Vec<DVec3>>> = Vec::new();
    for i in 0..n {
        let pn = joint_normal(i)?;
        let anchor = path[i % n];
        let ring: Option<Vec<Vec<DVec3>>> = if i == 0 {
            loops
                .iter()
                .map(|lp| project_ring(lp, dirs[0], anchor, pn))
                .collect()
        } else {
            rings[rings.len() - 1]
                .iter()
                .map(|lp| project_ring(lp, dirs[i - 1], anchor, pn))
                .collect()
        };
        rings.push(ring?);
    }
    Some((rings, dirs, spans))
}

/// The sweep as plain polygons for a live preview.
pub fn sweep_preview_faces(face: &PolyFace, path: &[DVec3], closed: bool) -> Vec<PolyFace> {
    let Some((rings, _, spans)) = sweep_rings(face, path, closed) else {
        return Vec::new();
    };
    let attrs = face.attrs.clone();
    let stations = rings.len();
    let mut out = Vec::new();
    for s in 0..spans {
        let r0 = &rings[s];
        let r1 = if closed {
            &rings[(s + 1) % stations]
        } else {
            &rings[s + 1]
        };
        for (lp0, lp1) in r0.iter().zip(r1.iter()) {
            let m = lp0.len();
            for j in 0..m {
                if let Some(q) = wall(&[lp0[j], lp0[(j + 1) % m], lp1[(j + 1) % m], lp1[j]]) {
                    out.push(PolyFace {
                        vertices: q,
                        holes: Vec::new(),
                        attrs: attrs.clone(),
                    });
                }
            }
        }
    }
    if !closed {
        let last = &rings[rings.len() - 1];
        out.push(PolyFace {
            vertices: last[0].clone(),
            holes: last[1..].to_vec(),
            attrs,
        });
    }
    out
}

/// The first station(s) of a path dragged from the profile over `edge`:
/// `(start_point, chain vertices)`.
pub fn manual_path_start(
    mesh: &Mesh,
    face: FaceId,
    edge: EdgeId,
    toward: Option<DVec3>,
) -> (Option<DVec3>, Vec<VertexId>) {
    let c = mesh.face_centroid(face);
    let n = mesh.face_normal(face).unit_or_zero();
    let (a, b) = (mesh.edge(edge).v0, mesh.edge(edge).v1);
    let (pa, pb) = (mesh.pos(a), mesh.pos(b));
    let da = (pa - c).dot(n);
    let db = (pb - c).dot(n);
    if da * db < -1e-12 {
        let t = da / (da - db);
        let p = pa + (pb - pa) * t;
        let mut far = b;
        if let Some(tw) = toward {
            if (pa - tw).length() < (pb - tw).length() {
                far = a;
            }
        }
        return (Some(p), vec![far]);
    }
    let (near, far) = if da.abs() <= db.abs() { (a, b) } else { (b, a) };
    (None, vec![near, far])
}

/// Grow — or shrink — a dragged path with the edge under the cursor.
pub fn manual_path_extend(
    mesh: &Mesh,
    chain: &mut Vec<VertexId>,
    edge: EdgeId,
    skip: &dyn Fn(EdgeId) -> bool,
    limit: usize,
) -> bool {
    if chain.is_empty() || skip(edge) {
        return false;
    }
    let last = chain[chain.len() - 1];
    let (v0, v1) = (mesh.edge(edge).v0, mesh.edge(edge).v1);
    let ids: HashMap<VertexId, usize> = chain.iter().enumerate().map(|(i, &v)| (v, i)).collect();
    if last == v0 || last == v1 {
        let other = if last == v0 { v1 } else { v0 };
        if chain.len() >= 2 && other == chain[chain.len() - 2] {
            chain.pop();
            return true;
        }
        if ids.contains_key(&other) {
            if other == chain[0] && chain.len() >= 3 {
                chain.push(other);
                return true;
            }
            return false;
        }
        chain.push(other);
        return true;
    }
    if let (Some(&i0), Some(&i1)) = (ids.get(&v0), ids.get(&v1)) {
        chain.truncate(i0.max(i1) + 1);
        return true;
    }
    let Some(route) = route(mesh, last, (v0, v1), skip, &ids, limit) else {
        return false;
    };
    chain.extend(route.iter().copied());
    let reached = route[route.len() - 1];
    let other = if reached == v0 { v1 } else { v0 };
    if !ids.contains_key(&other) && other != reached {
        chain.push(other);
    }
    true
}

fn route(
    mesh: &Mesh,
    start: VertexId,
    targets: (VertexId, VertexId),
    skip: &dyn Fn(EdgeId) -> bool,
    blocked: &HashMap<VertexId, usize>,
    limit: usize,
) -> Option<Vec<VertexId>> {
    let mut prev: HashMap<VertexId, Option<VertexId>> = HashMap::from([(start, None)]);
    let mut seen: HashSet<VertexId> = HashSet::from([start]);
    let mut q = VecDeque::from([start]);
    let mut hit = None;
    let mut steps = 0;
    while let Some(v) = q.pop_front() {
        if steps >= limit || hit.is_some() {
            break;
        }
        steps += 1;
        for &e in &mesh.verts[v].edges {
            if skip(e) {
                continue;
            }
            let w = mesh.edge(e).other(v);
            if seen.contains(&w) || (blocked.contains_key(&w) && w != start) {
                continue;
            }
            seen.insert(w);
            prev.insert(w, Some(v));
            if w == targets.0 || w == targets.1 {
                hit = Some(w);
                break;
            }
            q.push_back(w);
        }
    }
    let hit = hit?;
    let mut out = Vec::new();
    let mut v = Some(hit);
    while let Some(x) = v {
        if x == start {
            break;
        }
        out.push(x);
        v = prev.get(&x).copied().flatten();
    }
    out.reverse();
    Some(out)
}

/// A closed path as the sweep wants it: station 0 nearest the profile,
/// travelling the way whose first segment stands closest to perpendicular to
/// the profile.
pub fn orient_closed_path(pts: &[DVec3], face_centroid: DVec3, face_normal: DVec3) -> Vec<DVec3> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let k = (0..pts.len())
        .min_by(|&i, &j| {
            (pts[i] - face_centroid)
                .length()
                .partial_cmp(&(pts[j] - face_centroid).length())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let mut p: Vec<DVec3> = pts[k..].iter().chain(pts[..k].iter()).copied().collect();
    let n = face_normal.unit_or_zero();
    let fwd = p[1] - p[0];
    let back = p[p.len() - 1] - p[0];
    if fwd.length() > 1e-9 && back.length() > 1e-9 {
        let af = fwd.unit_or_zero().dot(n).abs();
        let ab = back.unit_or_zero().dot(n).abs();
        if ab > af + 1e-9 {
            let first = p[0];
            let mut rest: Vec<DVec3> = p[1..].to_vec();
            rest.reverse();
            p = std::iter::once(first).chain(rest).collect();
        }
    }
    p
}

/// Sweep `face` (with holes) along `path`, mutating `mesh`. `false` (mesh
/// untouched) on degenerate input.
pub fn sweep_profile(mesh: &mut Mesh, face: FaceId, path: &[DVec3], closed: bool) -> bool {
    let pf = mesh.poly_face(face);
    let Some((rings, _dirs, spans)) = sweep_rings(&pf, path, closed) else {
        return false;
    };
    let stations = path.len();
    let before_edges: HashSet<EdgeId> = mesh.edge_ids().into_iter().collect();
    let before_faces: HashSet<FaceId> = mesh.face_ids().into_iter().collect();
    let profile_loops: Vec<Vec<VertexId>> = mesh.face(face).loops().cloned().collect();
    mesh.remove_face(face);
    for s in 0..spans {
        let r0 = &rings[s];
        let r1 = if closed {
            &rings[(s + 1) % stations]
        } else {
            &rings[s + 1]
        };
        for (lp0, lp1) in r0.iter().zip(r1.iter()) {
            let m = lp0.len();
            for j in 0..m {
                if let Some(q) = wall(&[lp0[j], lp0[(j + 1) % m], lp1[(j + 1) % m], lp1[j]]) {
                    mesh.add_face(&q, &[]);
                }
            }
        }
    }
    if !closed {
        let first = &rings[0];
        let o: Vec<DVec3> = first[0].iter().rev().copied().collect();
        let hs: Vec<Vec<DVec3>> = first[1..]
            .iter()
            .map(|h| h.iter().rev().copied().collect())
            .collect();
        mesh.add_face(&o, &hs);
        let last = &rings[rings.len() - 1];
        mesh.add_face(&last[0], &last[1..]);
    }
    for lp in &profile_loops {
        let n = lp.len();
        for i in 0..n {
            if let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) {
                if mesh.edge(e).faces.is_empty() {
                    mesh.remove_edge(e);
                }
            }
        }
    }
    let seed: Vec<DVec3> = rings.iter().flatten().flatten().copied().collect();
    run_stitch(mesh, &seed, None, false, true);
    for e in mesh.edge_ids() {
        let ed = mesh.edge(e);
        if ed.soft || ed.faces.len() != 2 {
            continue;
        }
        if before_edges.contains(&e) && ed.faces.iter().any(|f| before_faces.contains(f)) {
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
    orient_outward(mesh, DEFAULT_SEED, None);
    true
}
