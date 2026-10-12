// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Fillet (round) the edges of a solid.
//!
//! An edge shared by two faces is replaced by a strip of quads on the
//! cylinder of the given radius tangent to both faces; the faces are cut
//! back to the tangent lines. Where the strip ends: an end cap (one more
//! face) cuts it by that face's plane; an open end cuts it square; two
//! rounded edges meeting stop on the mitre plane; three rounded edges (a box
//! corner) get a spherical patch. Anything else is refused with a reason.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::attrs::FaceAttrs;
use crate::math::{newell, DVec3};
use crate::mesh::{EdgeId, FaceId, Mesh, VertexId};
use crate::topology::point_in_face_solid;

/// Faces meeting at less than this angle (or more than 180° minus it) are
/// too flat or too sharp to round.
pub const MIN_ANGLE_DEG: f64 = 2.0;
/// Rebuilt loops may leave their face plane by this much (relative).
pub const PLANE_TOL: f64 = 1e-4;
/// Two computed points closer than this are the SAME point.
const SAME: f64 = 1e-5;

fn same(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < SAME
}

fn unit(v: DVec3) -> Option<DVec3> {
    let l = v.length();
    (l >= 1e-12).then(|| v / l)
}

fn plane_of(mesh: &Mesh, f: FaceId) -> (DVec3, DVec3) {
    let n = mesh.face_normal(f);
    let n = if n.length_squared() > 1e-18 {
        n.normalize()
    } else {
        n
    };
    (mesh.face_positions(f)[0], n)
}

/// Unit vector in `face`'s plane, perpendicular to the edge, pointing INTO
/// the face.
fn inward(mesh: &Mesh, face: FaceId, v0: VertexId, v1: VertexId) -> Option<DVec3> {
    let mut d = None;
    'loops: for lp in mesh.face(face).loops() {
        let n = lp.len();
        for i in 0..n {
            let (a, b) = (lp[i], lp[(i + 1) % n]);
            if a == v0 && b == v1 {
                d = Some(mesh.pos(v1) - mesh.pos(v0));
                break 'loops;
            }
            if a == v1 && b == v0 {
                d = Some(mesh.pos(v0) - mesh.pos(v1));
                break 'loops;
            }
        }
    }
    unit(mesh.face_normal(face).cross(d?))
}

#[derive(Clone, Debug)]
struct EdgeRound {
    edge: EdgeId,
    v0: VertexId,
    v1: VertexId,
    f1: FaceId,
    f2: FaceId,
    d: DVec3,
    w1: DVec3,
    w2: DVec3,
    t: f64,
    c_off: DVec3,
    offsets: Vec<DVec3>,
    sign: f64,
    ends: HashMap<VertexId, Vec<DVec3>>,
    end_kind: HashMap<VertexId, &'static str>,
}

impl EdgeRound {
    fn other(&self, v: VertexId) -> VertexId {
        if v == self.v0 {
            self.v1
        } else {
            self.v0
        }
    }
}

/// A rebuilt face: `(loop, holes, attrs, interior)`.
pub type Rebuilt = (Vec<DVec3>, Vec<Vec<DVec3>>, FaceAttrs, bool);

#[derive(Clone, Debug, Default)]
pub struct FilletPlan {
    pub radius: f64,
    pub segments: usize,
    pub removed_faces: Vec<FaceId>,
    pub removed_edges: Vec<EdgeId>,
    pub rebuilt: Vec<Rebuilt>,
    pub strips: Vec<Vec<DVec3>>,
    pub patches: Vec<Vec<DVec3>>,
    pub soft: Vec<(DVec3, DVec3)>,
    pub curves: Vec<Vec<DVec3>>,
    /// Attrs for the new faces of the strip.
    pub strip_attrs: Option<FaceAttrs>,
}

impl FilletPlan {
    pub fn new_faces(&self) -> Vec<Vec<DVec3>> {
        self.strips
            .iter()
            .chain(self.patches.iter())
            .cloned()
            .collect()
    }
}

fn section_offsets(
    w1: DVec3,
    w2: DVec3,
    radius: f64,
    segments: usize,
) -> Option<(f64, DVec3, Vec<DVec3>)> {
    let cos_a = w1.dot(w2).clamp(-1.0, 1.0);
    let alpha = cos_a.acos();
    let min = MIN_ANGLE_DEG.to_radians();
    if alpha < min || alpha > std::f64::consts::PI - min {
        return None;
    }
    let t = radius / (alpha / 2.0).tan();
    let c_off = (w1 + w2) * (radius / alpha.sin());
    let t1 = w1 * t;
    let t2 = w2 * t;
    let a = unit(t1 - c_off)?;
    let rb = t2 - c_off;
    let b = unit(rb - a * rb.dot(a))?;
    let phi = a.dot(unit(rb)?).clamp(-1.0, 1.0).acos();
    let pts = (0..=segments)
        .map(|k| {
            let ang = phi * k as f64 / segments as f64;
            c_off + (a * ang.cos() + b * ang.sin()) * radius
        })
        .collect();
    Some((t, c_off, pts))
}

fn cut(p: DVec3, offsets: &[DVec3], d: DVec3, plane: (DVec3, DVec3)) -> Option<Vec<DVec3>> {
    let (q, m) = plane;
    let denom = d.dot(m);
    if denom.abs() < 0.05 {
        return None;
    }
    Some(
        offsets
            .iter()
            .map(|&off| {
                let base = p + off;
                let s = (q - base).dot(m) / denom;
                base + d * s
            })
            .collect(),
    )
}

fn line_intersection_2d(p: DVec3, u: DVec3, q: DVec3, v: DVec3, normal: DVec3) -> Option<DVec3> {
    let den = u.cross(v).dot(normal);
    if den.abs() < 1e-12 {
        return None;
    }
    let s = (q - p).cross(v).dot(normal) / den;
    Some(p + u * s)
}

fn fmt(p: DVec3) -> String {
    format!("({:.2}, {:.2}, {:.2})", p.x, p.y, p.z)
}

fn shared_attrs(mesh: &Mesh, f1: FaceId, f2: FaceId) -> FaceAttrs {
    let (a, b) = (&mesh.face(f1).attrs.paint, &mesh.face(f2).attrs.paint);
    if a == b {
        FaceAttrs {
            paint: a.clone(),
            ..FaceAttrs::default()
        }
    } else {
        FaceAttrs::default()
    }
}

fn oriented(lp: Vec<DVec3>, centre: DVec3, sign: f64) -> Vec<DVec3> {
    let c = lp.iter().copied().sum::<DVec3>() / lp.len() as f64;
    if newell(&lp).dot(c - centre) * sign < 0.0 {
        lp.into_iter().rev().collect()
    } else {
        lp
    }
}

enum SetOutcome {
    Ok,
    Conflict,
    Outside,
}

/// Plan the rounding of `edges` with `radius`; `Err(reason)` when it
/// cannot be done.
pub fn plan_fillet(
    mesh: &Mesh,
    edges: &[EdgeId],
    radius: f64,
    segments: usize,
) -> Result<FilletPlan, String> {
    let mut uniq: Vec<EdgeId> = Vec::new();
    for &e in edges {
        if !uniq.contains(&e) && mesh.has_edge(e) {
            uniq.push(e);
        }
    }
    if uniq.is_empty() {
        return Err("Pick an edge to round.".into());
    }
    if radius <= 0.0 {
        return Err("The radius must be positive.".into());
    }
    let segments = segments.max(1);
    let mut rounds: IndexMap<EdgeId, EdgeRound> = IndexMap::new();
    for &e in &uniq {
        let ed = mesh.edge(e);
        if ed.faces.len() != 2 {
            return Err("Only an edge between exactly two faces can be rounded.".into());
        }
        let (f1, f2) = (ed.faces[0], ed.faces[1]);
        let d = unit(mesh.pos(ed.v1) - mesh.pos(ed.v0))
            .ok_or("A zero-length edge cannot be rounded.")?;
        let (Some(w1), Some(w2)) = (
            inward(mesh, f1, ed.v0, ed.v1),
            inward(mesh, f2, ed.v0, ed.v1),
        ) else {
            return Err("Could not read the faces around the edge.".into());
        };
        let (t, c_off, offsets) = section_offsets(w1, w2, radius, segments)
            .ok_or("The faces meet too flat or too sharp to round.")?;
        let n1 = mesh.face_normal(f1);
        let sign = if n1.dot(w1 * t - c_off) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        rounds.insert(
            e,
            EdgeRound {
                edge: e,
                v0: ed.v0,
                v1: ed.v1,
                f1,
                f2,
                d,
                w1,
                w2,
                t,
                c_off,
                offsets,
                sign,
                ends: HashMap::new(),
                end_kind: HashMap::new(),
            },
        );
    }
    let mut at_vertex: IndexMap<VertexId, Vec<EdgeId>> = IndexMap::new();
    for r in rounds.values() {
        for v in [r.v0, r.v1] {
            at_vertex.entry(v).or_default().push(r.edge);
        }
    }
    let mut replace: HashMap<(FaceId, VertexId), Vec<DVec3>> = HashMap::new();
    let mut plan = FilletPlan {
        radius,
        segments,
        ..FilletPlan::default()
    };
    let mut patch_corners: Vec<(Vec<EdgeId>, Vec<Vec<DVec3>>)> = Vec::new();

    let set_replacement = |replace: &mut HashMap<(FaceId, VertexId), Vec<DVec3>>,
                           face: FaceId,
                           v: VertexId,
                           pts: &[DVec3],
                           r: Option<&EdgeRound>|
     -> SetOutcome {
        if let Some(r) = r {
            if pts.len() == 1 && (face == r.f1 || face == r.f2) {
                let w = if face == r.f1 { r.w1 } else { r.w2 };
                let mid = (mesh.pos(r.v0) + mesh.pos(r.v1)) * 0.5;
                let p = pts[0];
                let q = p + ((mid - p) - w * r.t) * 1e-3;
                if !point_in_face_solid(&mesh.poly_face(face), q) {
                    return SetOutcome::Outside;
                }
            }
        }
        let key = (face, v);
        match replace.get(&key) {
            None => {
                replace.insert(key, pts.to_vec());
                SetOutcome::Ok
            }
            Some(cur) => {
                if pts.len() == 1 && cur.iter().any(|&p| same(p, pts[0])) {
                    return SetOutcome::Ok;
                }
                if cur.len() == 1 && pts.iter().any(|&p| same(p, cur[0])) {
                    replace.insert(key, pts.to_vec());
                    return SetOutcome::Ok;
                }
                if cur.len() == pts.len() && cur.iter().zip(pts.iter()).all(|(&a, &b)| same(a, b)) {
                    return SetOutcome::Ok;
                }
                SetOutcome::Conflict
            }
        }
    };
    let check = |outcomes: &[SetOutcome], p: DVec3| -> Result<(), String> {
        if outcomes.iter().any(|o| matches!(o, SetOutcome::Outside)) {
            return Err(format!(
                "The radius is too large for the face at {}.",
                fmt(p)
            ));
        }
        if outcomes.iter().any(|o| matches!(o, SetOutcome::Conflict)) {
            return Err(format!("Conflicting cuts at {}.", fmt(p)));
        }
        Ok(())
    };

    for (&v, group) in &at_vertex {
        let vp = mesh.pos(v);
        let faces_here: HashSet<FaceId> = mesh.vertex_faces(v).into_iter().collect();
        match group.len() {
            1 => {
                let r = rounds[&group[0]].clone();
                let caps: Vec<FaceId> = {
                    let mut c: Vec<FaceId> = faces_here
                        .iter()
                        .copied()
                        .filter(|&g| g != r.f1 && g != r.f2)
                        .collect();
                    c.sort();
                    c
                };
                if caps.len() > 1 {
                    return Err(format!(
                        "The corner at {} has too many faces to round.",
                        fmt(vp)
                    ));
                }
                let (plane, kind) = match caps.first() {
                    Some(&c) => (plane_of(mesh, c), "cap"),
                    None => ((vp, r.d), "open"),
                };
                let sec = cut(vp, &r.offsets, r.d, plane).ok_or_else(|| {
                    format!("The end face at {} is too oblique to the edge.", fmt(vp))
                })?;
                let mut outcome = vec![
                    set_replacement(&mut replace, r.f1, v, &[sec[0]], Some(&r)),
                    set_replacement(&mut replace, r.f2, v, &[sec[sec.len() - 1]], Some(&r)),
                ];
                if let Some(&c) = caps.first() {
                    outcome.push(set_replacement(&mut replace, c, v, &sec, Some(&r)));
                }
                check(&outcome, vp)?;
                let rr = rounds.get_mut(&group[0]).expect("round");
                rr.ends.insert(v, sec);
                rr.end_kind.insert(v, kind);
            }
            2 => {
                let ra = rounds[&group[0]].clone();
                let rb = rounds[&group[1]].clone();
                let allowed: HashSet<FaceId> = [ra.f1, ra.f2, rb.f1, rb.f2].into_iter().collect();
                if faces_here.iter().any(|g| !allowed.contains(g)) {
                    return Err(format!(
                        "The corner at {} has too many faces to round.",
                        fmt(vp)
                    ));
                }
                let (Some(u), Some(u2)) = (
                    unit(mesh.pos(ra.other(v)) - vp),
                    unit(mesh.pos(rb.other(v)) - vp),
                ) else {
                    return Err(format!(
                        "The two edges at {} fold back on each other.",
                        fmt(vp)
                    ));
                };
                let m = unit(u - u2).ok_or_else(|| {
                    format!("The two edges at {} fold back on each other.", fmt(vp))
                })?;
                let sec = cut(vp, &ra.offsets, ra.d, (vp, m)).ok_or_else(|| {
                    format!("The edges at {} meet too sharply to round.", fmt(vp))
                })?;
                let sec_b: Vec<DVec3> = if rb.f1 == ra.f1 || rb.f2 == ra.f2 {
                    sec.clone()
                } else if rb.f1 == ra.f2 || rb.f2 == ra.f1 {
                    sec.iter().rev().copied().collect()
                } else {
                    return Err(format!("The edges at {} share no face.", fmt(vp)));
                };
                let outcome = [
                    set_replacement(&mut replace, ra.f1, v, &[sec[0]], Some(&ra)),
                    set_replacement(&mut replace, ra.f2, v, &[sec[sec.len() - 1]], Some(&ra)),
                    set_replacement(&mut replace, rb.f1, v, &[sec_b[0]], Some(&rb)),
                    set_replacement(&mut replace, rb.f2, v, &[sec_b[sec_b.len() - 1]], Some(&rb)),
                ];
                check(&outcome, vp)?;
                let a = rounds.get_mut(&group[0]).expect("round");
                a.ends.insert(v, sec);
                a.end_kind.insert(v, "chain");
                let b = rounds.get_mut(&group[1]).expect("round");
                b.ends.insert(v, sec_b);
                b.end_kind.insert(v, "chain");
            }
            3 => {
                let rs: Vec<EdgeRound> = group.iter().map(|e| rounds[e].clone()).collect();
                let allowed: HashSet<FaceId> = rs.iter().flat_map(|r| [r.f1, r.f2]).collect();
                if faces_here.iter().any(|g| !allowed.contains(g)) {
                    return Err(format!(
                        "The corner at {} has too many faces to round.",
                        fmt(vp)
                    ));
                }
                let mut sections = Vec::new();
                for (ri, r) in rs.iter().enumerate() {
                    let mut cut_pts = Vec::new();
                    for (f, w) in [(r.f1, r.w1), (r.f2, r.w2)] {
                        let other: Vec<&EdgeRound> = rs
                            .iter()
                            .enumerate()
                            .filter(|(qi, q)| *qi != ri && (q.f1 == f || q.f2 == f))
                            .map(|(_, q)| q)
                            .collect();
                        if other.len() != 1 {
                            return Err(format!(
                                "The corner at {} is not a simple corner.",
                                fmt(vp)
                            ));
                        }
                        let q = other[0];
                        let wq = if q.f1 == f { q.w1 } else { q.w2 };
                        let normal = mesh.face_normal(f).normalize();
                        let hit =
                            line_intersection_2d(vp + w * r.t, r.d, vp + wq * q.t, q.d, normal)
                                .ok_or_else(|| {
                                    format!("The corner at {} is not a simple corner.", fmt(vp))
                                })?;
                        cut_pts.push(hit);
                    }
                    let s1 = (cut_pts[0] - vp).dot(r.d);
                    let s2 = (cut_pts[1] - vp).dot(r.d);
                    if (s1 - s2).abs() > 1e-3 * 1f64.max(r.t) {
                        return Err(format!("The corner at {} is too skewed to round.", fmt(vp)));
                    }
                    if s1.abs() > mesh.edge_length(r.edge) * 0.5 + 1e-9 {
                        return Err(format!(
                            "The radius is too large for the edge at {}.",
                            fmt(vp)
                        ));
                    }
                    let p = vp + r.d * s1;
                    let sec: Vec<DVec3> = r.offsets.iter().map(|&o| p + o).collect();
                    let outcome = [
                        set_replacement(&mut replace, r.f1, v, &[sec[0]], Some(r)),
                        set_replacement(&mut replace, r.f2, v, &[sec[sec.len() - 1]], Some(r)),
                    ];
                    check(&outcome, vp)?;
                    sections.push(sec);
                }
                for (r, sec) in rs.iter().zip(sections.iter()) {
                    let rr = rounds.get_mut(&r.edge).expect("round");
                    rr.ends.insert(v, sec.clone());
                    rr.end_kind.insert(v, "corner");
                }
                patch_corners.push((group.clone(), sections));
            }
            _ => {
                return Err(format!(
                    "More than three rounded edges meet at {}.",
                    fmt(vp)
                ))
            }
        }
    }

    // Rebuild every face that loses a vertex.
    let mut touched: IndexMap<FaceId, ()> = IndexMap::new();
    for &v in at_vertex.keys() {
        for f in mesh.vertex_faces(v) {
            touched.insert(f, ());
        }
    }
    let replaced: HashSet<VertexId> = at_vertex.keys().copied().collect();
    let rebuilt_loop = |face: FaceId, lp: &[VertexId]| -> Option<Vec<DVec3>> {
        let mut out = Vec::new();
        let n = lp.len();
        for (i, &x) in lp.iter().enumerate() {
            if !replaced.contains(&x) {
                out.push(mesh.pos(x));
                continue;
            }
            let mut seq = replace.get(&(face, x))?.clone();
            if seq.len() > 1 {
                let prev = mesh.pos(lp[(i + n - 1) % n]);
                let nxt = mesh.pos(lp[(i + 1) % n]);
                let straight = (prev - seq[0]).length() + (nxt - seq[seq.len() - 1]).length();
                let flipped = (prev - seq[seq.len() - 1]).length() + (nxt - seq[0]).length();
                if flipped < straight {
                    seq.reverse();
                }
            }
            out.extend(seq);
        }
        let mut clean: Vec<DVec3> = Vec::new();
        for p in out {
            if clean.last().is_none_or(|l| (p - *l).length() > 1e-7) {
                clean.push(p);
            }
        }
        if clean.len() > 1 && (clean[0] - clean[clean.len() - 1]).length() <= 1e-7 {
            clean.pop();
        }
        Some(clean)
    };
    for &f in touched.keys() {
        let (p0, normal) = plane_of(mesh, f);
        let lp = rebuilt_loop(f, &mesh.face(f).outer)
            .filter(|l| l.len() >= 3)
            .ok_or("A face would collapse — the radius is too large.")?;
        let mut holes = Vec::new();
        for h in &mesh.face(f).holes {
            let hl = rebuilt_loop(f, h)
                .filter(|l| l.len() >= 3)
                .ok_or("A face would collapse — the radius is too large.")?;
            holes.push(hl);
        }
        let span = lp.iter().map(|p| (*p - p0).length()).fold(0.0, f64::max);
        let span = if span == 0.0 { 1.0 } else { span };
        for &p in &lp {
            if (p - p0).dot(normal).abs() > PLANE_TOL * 1f64.max(span) {
                return Err(format!("Rounding would bend the face at {}.", fmt(p)));
            }
        }
        plan.removed_faces.push(f);
        plan.rebuilt
            .push((lp, holes, mesh.face(f).attrs.clone(), mesh.face(f).interior));
    }

    // Strips.
    for r in rounds.values() {
        let (Some(a), Some(b)) = (r.ends.get(&r.v0), r.ends.get(&r.v1)) else {
            return Err("Internal error: an edge end was not resolved.".into());
        };
        let along = (b[0] - a[0]).dot(r.d);
        if along <= 1e-6 {
            return Err(format!(
                "The radius is too large for the edge at {}.",
                fmt(mesh.pos(r.v0))
            ));
        }
        let n = a.len() - 1;
        let c_mid = (a[0] + b[0]) * 0.5 - r.w1 * r.t + r.c_off;
        for k in 0..n {
            plan.strips.push(oriented(
                vec![a[k], a[k + 1], b[k + 1], b[k]],
                c_mid,
                r.sign,
            ));
        }
        for k in 1..n {
            plan.soft.push((a[k], b[k]));
        }
        plan.soft.push((a[0], b[0]));
        plan.soft.push((a[n], b[n]));
        let mut end_vs: Vec<VertexId> = r.ends.keys().copied().collect();
        end_vs.sort();
        for vid in end_vs {
            let sec = &r.ends[&vid];
            let kind = r.end_kind[&vid];
            if kind == "chain" || kind == "corner" {
                for k in 0..n {
                    plan.soft.push((sec[k], sec[k + 1]));
                }
            } else {
                plan.curves.push(sec.clone());
            }
        }
        if plan.strip_attrs.is_none() {
            plan.strip_attrs = Some(shared_attrs(mesh, r.f1, r.f2));
        }
        plan.removed_edges.push(r.edge);
    }

    // Corner patches.
    for (group, sections) in &patch_corners {
        let rs: Vec<&EdgeRound> = group.iter().map(|e| &rounds[e]).collect();
        let Some(CornerPatch { tris, soft }) = corner_patch(&rs, sections, radius, segments) else {
            return Err(format!(
                "Could not close the corner at {}.",
                fmt(sections[0][0])
            ));
        };
        plan.patches.extend(tris);
        plan.soft.extend(soft);
    }

    let mut seen: HashSet<EdgeId> = plan.removed_edges.iter().copied().collect();
    for &v in at_vertex.keys() {
        for &e in &mesh.verts[v].edges {
            if seen.contains(&e) || mesh.edge(e).faces.is_empty() {
                continue;
            }
            seen.insert(e);
            plan.removed_edges.push(e);
        }
    }
    Ok(plan)
}

/// The surface closing a corner where three rounded edges meet.
struct CornerPatch {
    tris: Vec<Vec<DVec3>>,
    /// The patch's inner edges, drawn soft.
    soft: Vec<(DVec3, DVec3)>,
}

fn corner_patch(
    group: &[&EdgeRound],
    sections: &[Vec<DVec3>],
    radius: f64,
    segments: usize,
) -> Option<CornerPatch> {
    let n = segments;
    let (s1, s2, s3) = (&sections[0], &sections[1], &sections[2]);
    let meet = |sa: &[DVec3], sb: &[DVec3]| -> Option<DVec3> {
        for &i in &[0usize, sa.len() - 1] {
            for &j in &[0usize, sb.len() - 1] {
                if (sa[i] - sb[j]).length() < 1e-5 {
                    return Some(sa[i]);
                }
            }
        }
        None
    };
    let c12 = meet(s1, s2)?;
    let c23 = meet(s2, s3)?;
    let c31 = meet(s3, s1)?;
    let mut centre = DVec3::ZERO;
    for (r, s) in group.iter().zip(sections.iter()) {
        centre += s[0] - r.w1 * r.t + r.c_off;
    }
    centre /= 3.0;
    let ea = unit(c12 - centre)?;
    let eb = unit(c23 - centre)?;
    let ec = unit(c31 - centre)?;
    let oriented_arc = |sec: &Vec<DVec3>, start: DVec3, end: DVec3| -> Option<Vec<DVec3>> {
        if (sec[0] - start).length() < 1e-5 && (sec[sec.len() - 1] - end).length() < 1e-5 {
            return Some(sec.clone());
        }
        if (sec[sec.len() - 1] - start).length() < 1e-5 && (sec[0] - end).length() < 1e-5 {
            return Some(sec.iter().rev().copied().collect());
        }
        None
    };
    let mut arcs: HashMap<&str, Vec<DVec3>> = HashMap::new();
    for s in [s1, s2, s3] {
        for (name, (p, q)) in [("ab", (c12, c23)), ("bc", (c23, c31)), ("ac", (c12, c31))] {
            if let Some(a) = oriented_arc(s, p, q) {
                arcs.entry(name).or_insert(a);
            }
        }
    }
    if arcs.len() != 3 {
        return None;
    }
    let f = |i: usize| (i as f64 * std::f64::consts::PI / (2.0 * n as f64)).sin();
    let mut grid: IndexMap<(usize, usize), DVec3> = IndexMap::new();
    for i in 0..=n {
        for j in 0..=(n - i) {
            let k = n - i - j;
            let p = if k == 0 {
                arcs["ab"][j]
            } else if i == 0 {
                arcs["bc"][k]
            } else if j == 0 {
                arcs["ac"][k]
            } else {
                centre + unit(ea * f(i) + eb * f(j) + ec * f(k))? * radius
            };
            grid.insert((i, j), p);
        }
    }
    let sign = group[0].sign;
    let mut tris = Vec::new();
    for i in 0..n {
        for j in 0..(n - i) {
            tris.push(oriented(
                vec![grid[&(i, j)], grid[&(i + 1, j)], grid[&(i, j + 1)]],
                centre,
                sign,
            ));
            if i + j + 1 < n {
                tris.push(oriented(
                    vec![grid[&(i + 1, j)], grid[&(i + 1, j + 1)], grid[&(i, j + 1)]],
                    centre,
                    sign,
                ));
            }
        }
    }
    let mut soft = Vec::new();
    for (&(i, j), &p) in &grid {
        for (di, dj) in [(1i64, 0i64), (0, 1), (-1, 1)] {
            let (ni, nj) = (i as i64 + di, j as i64 + dj);
            if ni < 0 || nj < 0 {
                continue;
            }
            if let Some(&q) = grid.get(&(ni as usize, nj as usize)) {
                soft.push((p, q));
            }
        }
    }
    Some(CornerPatch { tris, soft })
}

/// Apply a plan to the mesh.
pub fn apply_fillet(mesh: &mut Mesh, plan: &FilletPlan) {
    for &f in &plan.removed_faces {
        mesh.remove_face(f);
    }
    for &e in &plan.removed_edges {
        if mesh.has_edge(e) && mesh.edge(e).faces.is_empty() {
            mesh.remove_edge(e);
        }
    }
    for (lp, holes, attrs, interior) in &plan.rebuilt {
        let f = mesh.add_face(lp, holes);
        mesh.faces[f].attrs = attrs.clone();
        mesh.faces[f].interior = *interior;
    }
    let attrs = plan.strip_attrs.clone().unwrap_or_default();
    for lp in plan.new_faces() {
        let f = mesh.add_face(&lp, &[]);
        if !attrs.is_empty() {
            mesh.faces[f].attrs = attrs.clone();
        }
    }
    for &(p, q) in &plan.soft {
        if let (Some(a), Some(b)) = (mesh.vertex_at(p), mesh.vertex_at(q)) {
            if let Some(e) = mesh.find_edge(a, b) {
                mesh.edges[e].soft = true;
            }
        }
    }
    for pts in &plan.curves {
        mesh.tag_curve(pts, false);
    }
}
