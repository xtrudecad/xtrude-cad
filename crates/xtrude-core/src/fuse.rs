// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Fast coplanar fusion for imported triangle soups (it lives in the
//! kernel because Simplify Mesh uses it on any mesh).
//!
//! - union-find the loops into connected regions via shared welded edges
//!   (same material, near-identical normals, with a drift guard);
//! - a region's boundary is the edges used exactly once — traced into
//!   closed loops; the largest is the outer contour, the rest are holes.
//!
//! Anything irregular bails out to the original loops for that region.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::attrs::FaceAttrs;
use crate::math::{newell, DVec3};
use crate::mesh::{key, Key, Mesh};

/// A fused region: `(outer, holes, attrs, originals)`.
pub type Fused = (
    Vec<DVec3>,
    Vec<Vec<DVec3>>,
    Option<FaceAttrs>,
    Vec<Vec<DVec3>>,
);

pub type Loop = (Vec<DVec3>, Option<FaceAttrs>);

fn ukey(a: Key, b: Key) -> (Key, Key) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Merge coplanar same-material connected regions of `loops` into polygons
/// (holes included); everything else passes through unchanged.
pub fn fuse_coplanar_loops(loops: &[Loop], cos_tol: f64, principal_planes: bool) -> Vec<Fused> {
    struct F {
        pts: Vec<DVec3>,
        attrs: Option<FaceAttrs>,
        n: DVec3,
    }
    let mut faces: Vec<F> = Vec::new();
    let mut seen_tris: HashSet<Vec<Key>> = HashSet::new();
    for (pts, attrs) in loops {
        if pts.len() < 3 {
            continue;
        }
        let n = newell(pts);
        let ln = n.length();
        if ln < 1e-10 {
            continue; // degenerate sliver
        }
        let attrs = attrs.clone().filter(|a| !a.is_empty());
        if pts.len() == 3 {
            let mut tk: Vec<Key> = pts.iter().map(|&p| key(p)).collect();
            tk.sort();
            tk.dedup();
            if !seen_tris.insert(tk) {
                continue; // coincident duplicate (two-sided export)
            }
        }
        faces.push(F {
            pts: pts.clone(),
            attrs,
            n: n / ln,
        });
    }
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut edge_map: IndexMap<(Key, Key), Vec<usize>> = IndexMap::new();
    for (i, f) in faces.iter().enumerate() {
        let m = f.pts.len();
        for k in 0..m {
            let (a, b) = (key(f.pts[k]), key(f.pts[(k + 1) % m]));
            if a != b {
                edge_map.entry(ukey(a, b)).or_default().push(i);
            }
        }
    }
    let principal = |n: DVec3| n.x.abs().max(n.y.abs()).max(n.z.abs()) >= 1.0 - 1e-7;
    let root_n: Vec<DVec3> = faces.iter().map(|f| f.n).collect();
    for idxs in edge_map.values() {
        if idxs.len() != 2 {
            continue;
        }
        let (i, j) = (idxs[0], idxs[1]);
        if principal_planes && !(principal(faces[i].n) && principal(faces[j].n)) {
            continue;
        }
        if faces[i].attrs != faces[j].attrs {
            continue;
        }
        if faces[i].n.dot(faces[j].n).abs() < cos_tol {
            continue;
        }
        let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
        if ri == rj {
            continue;
        }
        if root_n[ri].dot(root_n[rj]).abs() < cos_tol {
            continue; // drift guard: stay planar
        }
        if faces[ri].attrs != faces[rj].attrs {
            continue;
        }
        parent[rj] = ri;
    }
    let mut regions: IndexMap<usize, Vec<usize>> = IndexMap::new();
    for i in 0..faces.len() {
        let r = find(&mut parent, i);
        regions.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for region in regions.values() {
        let loops: Vec<(Vec<DVec3>, Option<FaceAttrs>, DVec3)> = region
            .iter()
            .map(|&i| (faces[i].pts.clone(), faces[i].attrs.clone(), faces[i].n))
            .collect();
        match trace_region(&loops) {
            Some(f) => out.push(f),
            None => {
                for (pts, attrs, _) in loops {
                    out.push((pts.clone(), Vec::new(), attrs, vec![pts]));
                }
            }
        }
    }
    out
}

fn trace_region(loops: &[(Vec<DVec3>, Option<FaceAttrs>, DVec3)]) -> Option<Fused> {
    if loops.len() == 1 {
        let (pts, attrs, _) = &loops[0];
        return Some((pts.clone(), Vec::new(), attrs.clone(), vec![pts.clone()]));
    }
    let mut count: IndexMap<(Key, Key), usize> = IndexMap::new();
    let mut pos: HashMap<Key, DVec3> = HashMap::new();
    for (pts, _, _) in loops {
        let m = pts.len();
        for k in 0..m {
            let (ka, kb) = (key(pts[k]), key(pts[(k + 1) % m]));
            if ka == kb {
                continue;
            }
            *count.entry(ukey(ka, kb)).or_insert(0) += 1;
            pos.entry(ka).or_insert(pts[k]);
            pos.entry(kb).or_insert(pts[(k + 1) % m]);
        }
    }
    let boundary: Vec<(Key, Key)> = count
        .iter()
        .filter(|(_, &c)| c == 1)
        .map(|(&e, _)| e)
        .collect();
    if boundary.is_empty() || count.values().any(|&c| c > 2) {
        return None;
    }
    let mut adj: IndexMap<Key, Vec<Key>> = IndexMap::new();
    for &(a, b) in &boundary {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    if adj.values().any(|v| v.len() != 2) {
        return None;
    }
    let mut unvisited: indexmap::IndexSet<(Key, Key)> = boundary.iter().copied().collect();
    let mut traced: Vec<Vec<DVec3>> = Vec::new();
    while let Some(&e0) = unvisited.first() {
        let (start, mut cur) = e0;
        unvisited.shift_remove(&e0);
        let mut walk = vec![start, cur];
        loop {
            let nb = &adj[&cur];
            let prev = walk[walk.len() - 2];
            let nxt = if nb[0] == prev { nb[1] } else { nb[0] };
            let edge = ukey(cur, nxt);
            if !unvisited.contains(&edge) {
                break;
            }
            unvisited.shift_remove(&edge);
            if nxt == start {
                break;
            }
            walk.push(nxt);
            cur = nxt;
        }
        if walk.last() == Some(&start) {
            walk.pop();
        }
        if walk.len() < 3 {
            return None;
        }
        traced.push(walk.iter().map(|k| pos[k]).collect());
    }
    let n_ref = loops[0].2;
    let mut areas: Vec<(DVec3, Vec<DVec3>)> =
        traced.into_iter().map(|lp| (newell(&lp), lp)).collect();
    areas.sort_by(|a, b| {
        b.0.length()
            .partial_cmp(&a.0.length())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (outer_n, mut outer) = areas[0].clone();
    if outer_n.dot(n_ref) < 0.0 {
        outer.reverse();
    }
    let mut holes = Vec::new();
    for (hn, mut lp) in areas.into_iter().skip(1) {
        if hn.dot(n_ref) > 0.0 {
            lp.reverse();
        }
        holes.push(lp);
    }
    Some((
        outer,
        holes,
        loops[0].1.clone(),
        loops.iter().map(|l| l.0.clone()).collect(),
    ))
}

#[derive(Debug, thiserror::Error)]
#[error("Simplification angle must be between 0 and 5 degrees")]
pub struct BadAngle;

/// Merge connected near-coplanar faces in place; returns faces removed.
/// Panics never: an out-of-range angle merges nothing (see
/// [`check_simplify_angle`]).
pub fn simplify_mesh(mesh: &mut Mesh, max_angle_degrees: f64) -> usize {
    simplify_mesh_opts(mesh, max_angle_degrees, false).unwrap_or(0)
}

pub fn check_simplify_angle(max_angle_degrees: f64) -> Result<(), BadAngle> {
    if !max_angle_degrees.is_finite() || !(0.0..=5.0).contains(&max_angle_degrees) {
        return Err(BadAngle);
    }
    Ok(())
}

pub fn simplify_mesh_opts(
    mesh: &mut Mesh,
    max_angle_degrees: f64,
    principal_planes: bool,
) -> Result<usize, BadAngle> {
    check_simplify_angle(max_angle_degrees)?;
    if mesh.faces.is_empty() {
        return Ok(0);
    }
    let original = mesh.faces.len();
    let mut loops: Vec<Loop> = Vec::new();
    let mut holed = Vec::new();
    for (fid, f) in mesh.faces.iter() {
        if !f.holes.is_empty() || f.interior {
            holed.push(fid);
        } else {
            loops.push((
                mesh.face_positions(fid),
                Some(f.attrs.clone()).filter(|a| !a.is_empty()),
            ));
        }
    }
    let tolerance = max_angle_degrees.to_radians().max(0.00045);
    let fused = fuse_coplanar_loops(&loops, tolerance.cos(), principal_planes);
    let mut simplified = Mesh::new();
    for (outer, holes, attrs, _originals) in fused {
        let f = simplified.add_face(&outer, &holes);
        if let Some(a) = attrs {
            simplified.faces[f].attrs.update(&a);
        }
    }
    for fid in holed {
        let f = simplified.add_face(&mesh.face_positions(fid), &mesh.face_hole_positions(fid));
        simplified.faces[f].attrs.update(&mesh.face(fid).attrs);
        simplified.faces[f].interior = mesh.face(fid).interior;
    }
    let mut original_flags = HashMap::new();
    for (eid, e) in mesh.edges.iter() {
        let (a, b) = mesh.edge_ab(eid);
        original_flags.insert(ukey(key(a), key(b)), e.flags());
    }
    for eid in simplified.edge_ids() {
        let (a, b) = simplified.edge_ab(eid);
        if let Some(fl) = original_flags.get(&ukey(key(a), key(b))) {
            simplified.edge_mut(eid).stamp_flags(fl);
        }
    }
    loop {
        let mut collapsed = false;
        for v in simplified.vertex_ids() {
            if simplified.has_vertex(v) && simplified.collapsible_vertex(v) {
                simplified.collapse_vertex(v);
                collapsed = true;
                break;
            }
        }
        if !collapsed {
            break;
        }
    }
    soften_smooth_edges(&mut simplified, 0.85);
    *mesh = simplified;
    Ok(original.saturating_sub(mesh.faces.len()))
}

fn smooth_sig(a: &FaceAttrs) -> Option<[f64; 3]> {
    a.paint.color
}

/// Soften the edges the FILE itself calls smooth (OBJ `s` groups), keeping
/// material boundaries and genuine creases.
pub fn soften_by_smoothing_group(mesh: &mut Mesh, crease_cos: f64) {
    for e in mesh.edge_ids() {
        let ed = mesh.edge(e);
        if ed.soft || ed.faces.len() != 2 {
            continue;
        }
        let (f0, f1) = (ed.faces[0], ed.faces[1]);
        if f0 == f1 {
            continue;
        }
        let (a0, a1) = (&mesh.face(f0).attrs, &mesh.face(f1).attrs);
        if a0.is_empty() || a1.is_empty() {
            continue;
        }
        let Some(g) = a0.smooth.as_ref().filter(|g| !g.is_empty()) else {
            continue;
        };
        if Some(g) != a1.smooth.as_ref() {
            continue;
        }
        if smooth_sig(a0) != smooth_sig(a1) {
            continue;
        }
        let (n0, n1) = (mesh.face_normal(f0), mesh.face_normal(f1));
        if n0.length() < 1e-9 || n1.length() < 1e-9 || n0.dot(n1).abs() > crease_cos {
            mesh.edge_mut(e).soft = true;
        }
    }
}

/// Forget the file's smoothing groups once they have been used.
pub fn drop_smoothing_groups(mesh: &mut Mesh) {
    for f in mesh.face_ids() {
        if mesh.face(f).attrs.smooth.is_some() {
            mesh.face_mut(f).attrs.smooth = None;
        }
    }
}

/// Mark edges between two same-material faces meeting at a shallow
/// dihedral as soft — the classic import smoothing.
pub fn soften_smooth_edges(mesh: &mut Mesh, cos_threshold: f64) {
    let mut normals: HashMap<crate::FaceId, DVec3> = HashMap::new();
    for e in mesh.edge_ids() {
        let ed = mesh.edge(e);
        if ed.faces.len() != 2 {
            continue;
        }
        let (f0, f1) = (ed.faces[0], ed.faces[1]);
        if f0 == f1 {
            continue;
        }
        if smooth_sig(&mesh.face(f0).attrs) != smooth_sig(&mesh.face(f1).attrs) {
            continue;
        }
        let n0 = *normals.entry(f0).or_insert_with(|| mesh.face_normal(f0));
        let n1 = *normals.entry(f1).or_insert_with(|| mesh.face_normal(f1));
        if n0.dot(n1).abs() > cos_threshold {
            mesh.edge_mut(e).soft = true;
        }
    }
}
