// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Solid booleans: Merge Solids, Union, Subtract, Trim, Intersect and
//! Split.
//!
//! The arithmetic is Manifold (Apache-2.0), via the `manifold-csg`
//! bindings.
//! Each triangle carries the id of the face it came from — here as a fourth
//! vertex property, with explicit merge pairs so the shared positions stay
//! one manifold vertex — which is how materials survive; the result's
//! triangles are merged back into clean polygons with holes by
//! [`crate::fuse`], and the seams of curved surfaces stay soft.
//!
//! This module works on world-space meshes; the group bookkeeping (which
//! groups leave, where the results land) is the document's.

use std::collections::{HashMap, HashSet};

use manifold_csg::{Manifold, MeshGL64, MeshGL64Options, OpType};

use crate::attrs::FaceAttrs;
use crate::fuse::fuse_coplanar_loops;
use crate::math::DVec3;
use crate::mesh::{FaceId, Mesh};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidOp {
    MergeSolids,
    Union,
    Subtract,
    Trim,
    Intersect,
    Split,
}

impl SolidOp {
    pub const ALL: [SolidOp; 6] = [
        SolidOp::MergeSolids,
        SolidOp::Union,
        SolidOp::Subtract,
        SolidOp::Trim,
        SolidOp::Intersect,
        SolidOp::Split,
    ];

    /// The group name given to the results.
    pub fn result_name(self) -> &'static str {
        match self {
            SolidOp::MergeSolids => "Merged solids",
            SolidOp::Union => "Union",
            SolidOp::Subtract => "Difference",
            SolidOp::Trim => "Trimmed",
            SolidOp::Intersect => "Intersection",
            SolidOp::Split => "Split",
        }
    }

    /// The op of a key (`"union"`, `"subtract"`, …).
    pub fn from_key(key: &str) -> Option<SolidOp> {
        SolidOp::ALL.into_iter().find(|op| op.key() == key)
    }

    pub fn key(self) -> &'static str {
        match self {
            SolidOp::MergeSolids => "merge_solids",
            SolidOp::Union => "union",
            SolidOp::Subtract => "subtract",
            SolidOp::Trim => "trim",
            SolidOp::Intersect => "intersect",
            SolidOp::Split => "split",
        }
    }
}

/// An operation that cannot run; the message is for the user.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct SolidError(pub String);

/// `(is_solid, volume)`: every edge between exactly two (boundary) faces,
/// at least one face, positive volume.
pub fn solid_report(mesh: &Mesh) -> (bool, Option<f64>) {
    if mesh.faces.is_empty() {
        return (false, None);
    }
    for (_, e) in mesh.edges.iter() {
        let n = e.faces.iter().filter(|&&f| !mesh.face(f).interior).count();
        if n != 2 {
            return (false, None);
        }
    }
    let faces: Vec<FaceId> = mesh
        .face_ids()
        .into_iter()
        .filter(|&f| !mesh.face(f).interior)
        .collect();
    match crate::measure::face_set_volume(mesh, &faces) {
        Some(v) if v > 1e-12 => (true, Some(v)),
        _ => (false, None),
    }
}

/// Every source face, globally numbered across the inputs.
#[derive(Default)]
pub struct Sources {
    pub attrs: Vec<FaceAttrs>,
    pub soft_pairs: HashSet<(usize, usize)>,
}

/// A world-space solid input: its mesh and the paint its default faces
/// wear (the container's).
pub struct SolidInput<'a> {
    pub mesh: &'a Mesh,
    pub paint: Option<&'a crate::attrs::Paint>,
}

fn effective(attrs: &FaceAttrs, paint: Option<&crate::attrs::Paint>) -> FaceAttrs {
    let Some(p) = paint else { return attrs.clone() };
    if p.is_empty() || attrs.paint.color.is_some() {
        return attrs.clone();
    }
    let mut m = attrs.clone();
    if p.color.is_some() {
        m.paint.color = p.color;
    }
    if p.opacity.is_some() {
        m.paint.opacity = p.opacity;
    }
    if p.mat.is_some() {
        m.paint.mat = p.mat.clone();
    }
    if m.back.is_none() {
        m.back = Some(crate::attrs::Back::Mirror);
    }
    m
}

fn to_manifold(input: &SolidInput, sources: &mut Sources) -> Result<Manifold, SolidError> {
    let mesh = input.mesh;
    let mut pos_index: HashMap<(u64, u64, u64), u64> = HashMap::new();
    let mut prop_index: HashMap<((u64, u64, u64), usize), u64> = HashMap::new();
    let mut props: Vec<f64> = Vec::new();
    let mut tris: Vec<u64> = Vec::new();
    let mut merge_from: Vec<u64> = Vec::new();
    let mut merge_to: Vec<u64> = Vec::new();
    let mut face_src: HashMap<FaceId, usize> = HashMap::new();
    let mut tri_corners: Vec<[DVec3; 3]> = Vec::new();
    let mut tri_fids: Vec<usize> = Vec::new();
    for (f, face) in mesh.faces.iter() {
        if face.interior {
            continue;
        }
        let fid = sources.attrs.len();
        face_src.insert(f, fid);
        sources.attrs.push(effective(&face.attrs, input.paint));
        let n = mesh.face_normal(f);
        for t in mesh.face_triangulate(f, Some(n)) {
            tri_corners.push(t);
            tri_fids.push(fid);
        }
    }
    for (_, e) in mesh.edges.iter() {
        if e.soft {
            let fs: Vec<usize> = e
                .faces
                .iter()
                .filter_map(|f| face_src.get(f).copied())
                .collect();
            if fs.len() == 2 {
                sources
                    .soft_pairs
                    .insert((fs[0].min(fs[1]), fs[0].max(fs[1])));
            }
        }
    }
    if tri_corners.is_empty() {
        return Err(SolidError("empty".into()));
    }
    // Outward orientation: a negative signed volume means the faces point in.
    let vol: f64 = tri_corners.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum();
    let flip = vol < 0.0;
    let bits = |p: DVec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    for (t, &fid) in tri_corners.iter().zip(tri_fids.iter()) {
        let order: [usize; 3] = if flip { [0, 2, 1] } else { [0, 1, 2] };
        for &k in &order {
            let p = t[k];
            let key = bits(p);
            let idx = *prop_index.entry((key, fid)).or_insert_with(|| {
                let i = (props.len() / 4) as u64;
                props.extend_from_slice(&[p.x, p.y, p.z, fid as f64]);
                match pos_index.get(&key) {
                    Some(&canon) => {
                        merge_from.push(i);
                        merge_to.push(canon);
                    }
                    None => {
                        pos_index.insert(key, i);
                    }
                }
                i
            });
            tris.push(idx);
        }
    }
    let opts = MeshGL64Options::new().merge_vertices(&merge_from, &merge_to);
    let gl = MeshGL64::new_with_options(&props, 4, &tris, opts)
        .map_err(|e| SolidError(format!("{e}")))?;
    Manifold::from_meshgl64(&gl).map_err(|_| SolidError("not manifold".into()))
}

type Loop = (Vec<DVec3>, FaceAttrs, usize);

/// A fused face: outer loop, holes and attributes.
type Poly = (Vec<DVec3>, Vec<Vec<DVec3>>, Option<FaceAttrs>);

fn loops_from(man: &Manifold, sources: &Sources, drop_voids: bool) -> Vec<Loop> {
    let gl = man.to_meshgl64();
    let np = gl.num_prop().max(3);
    let vp = gl.vert_properties();
    let tv = gl.tri_verts();
    let nt = tv.len() / 3;
    if nt == 0 {
        return Vec::new();
    }
    let pos = |i: u64| {
        let b = i as usize * np;
        DVec3::new(vp[b], vp[b + 1], vp[b + 2])
    };
    let fid_of = |i: u64| -> usize {
        if np >= 4 {
            let v = vp[i as usize * np + 3];
            if v >= 0.0 {
                return v.round() as usize;
            }
        }
        usize::MAX
    };
    let keep = if drop_voids {
        outside_shells_mask(&gl, np, &vp, &tv)
    } else {
        vec![true; nt]
    };
    let mut out = Vec::new();
    for t in 0..nt {
        if !keep[t] {
            continue;
        }
        let (i, j, k) = (tv[3 * t], tv[3 * t + 1], tv[3 * t + 2]);
        let f = fid_of(i);
        let attrs = sources.attrs.get(f).cloned().unwrap_or_default();
        out.push((vec![pos(i), pos(j), pos(k)], attrs, f));
    }
    out
}

/// Keep the triangles of the shells that bound volume from outside: an
/// inner void is a connected shell of negative signed volume.
fn outside_shells_mask(_gl: &MeshGL64, np: usize, vp: &[f64], tv: &[u64]) -> Vec<bool> {
    let nt = tv.len() / 3;
    let nv = vp.len() / np;
    // Union-find on POSITIONS (property vertices sharing a position are one).
    let key = |i: usize| {
        (
            vp[i * np].to_bits(),
            vp[i * np + 1].to_bits(),
            vp[i * np + 2].to_bits(),
        )
    };
    let mut canon: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let mut rep: Vec<usize> = vec![0; nv];
    for (i, r) in rep.iter_mut().enumerate() {
        *r = *canon.entry(key(i)).or_insert(i);
    }
    let mut parent: Vec<usize> = (0..nv).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for t in 0..nt {
        let a = rep[tv[3 * t] as usize];
        let b = rep[tv[3 * t + 1] as usize];
        let c = rep[tv[3 * t + 2] as usize];
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[rb] = ra;
        let rc = find(&mut parent, c);
        parent[rc] = ra;
    }
    let p3 = |i: u64| {
        let b = i as usize * np;
        DVec3::new(vp[b], vp[b + 1], vp[b + 2])
    };
    let roots: Vec<usize> = (0..nt)
        .map(|t| find(&mut parent, rep[tv[3 * t] as usize]))
        .collect();
    let mut vol: HashMap<usize, f64> = HashMap::new();
    for t in 0..nt {
        let (a, b, c) = (p3(tv[3 * t]), p3(tv[3 * t + 1]), p3(tv[3 * t + 2]));
        *vol.entry(roots[t]).or_insert(0.0) += a.dot(b.cross(c));
    }
    roots.iter().map(|r| vol[r] >= 0.0).collect()
}

/// A result mesh with clean polygon faces (coplanar same-material
/// triangles merged, collinear corners dropped, soft seams kept soft).
fn mesh_from_loops(loops: Vec<Loop>, sources: &Sources) -> Mesh {
    // Carry the source id through the fuse inside the attrs' smoothing slot
    // (private, cleared after).
    let tagged: Vec<(Vec<DVec3>, Option<FaceAttrs>)> = loops
        .into_iter()
        .map(|(pts, mut a, f)| {
            a.smooth = Some(format!("solid:{f}"));
            (pts, Some(a))
        })
        .collect();
    let fused = fuse_coplanar_loops(&tagged, 0.99999, false);
    let mut polys: Vec<Poly> = fused.into_iter().map(|(o, h, a, _)| (o, h, a)).collect();
    drop_collinear(&mut polys);
    let mut mesh = Mesh::new();
    let mut face_src: HashMap<FaceId, usize> = HashMap::new();
    for (outer, holes, attrs) in polys {
        let mut clean = attrs.unwrap_or_default();
        let src = clean.smooth.take().and_then(|s| {
            s.strip_prefix("solid:")
                .and_then(|n| n.parse::<usize>().ok())
        });
        let f = mesh.add_face(&outer, &holes);
        mesh.faces[f].attrs.update(&clean);
        if let Some(s) = src {
            face_src.insert(f, s);
        }
    }
    for e in mesh.edge_ids() {
        let fs: Vec<Option<usize>> = mesh
            .edge(e)
            .faces
            .iter()
            .map(|f| face_src.get(f).copied())
            .collect();
        if fs.len() == 2 {
            if let (Some(a), Some(b)) = (fs[0], fs[1]) {
                if sources.soft_pairs.contains(&(a.min(b), a.max(b))) {
                    mesh.edge_mut(e).soft = true;
                }
            }
        }
    }
    mesh
}

fn drop_collinear(polys: &mut [Poly]) {
    let tol = 1e-9;
    let key = |p: DVec3| {
        (
            crate::math::round_even_int(p.x * 1e7),
            crate::math::round_even_int(p.y * 1e7),
            crate::math::round_even_int(p.z * 1e7),
        )
    };
    let mut straight: HashMap<(i64, i64, i64), bool> = HashMap::new();
    for (outer, holes, _) in polys.iter() {
        for ring in std::iter::once(outer).chain(holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let (p0, p1, p2) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
                let (d1, d2) = (p1 - p0, p2 - p1);
                let ok = d1.cross(d2).length()
                    <= tol * (d1.length() * d2.length()).max(1e-12) * 1e3
                    && d1.dot(d2) > 0.0;
                let e = straight.entry(key(p1)).or_insert(true);
                *e = *e && ok;
            }
        }
    }
    let drop: HashSet<(i64, i64, i64)> = straight
        .into_iter()
        .filter(|(_, v)| *v)
        .map(|(k, _)| k)
        .collect();
    if drop.is_empty() {
        return;
    }
    for (outer, holes, _) in polys.iter_mut() {
        for ring in std::iter::once(outer).chain(holes.iter_mut()) {
            let remaining = ring.iter().filter(|p| !drop.contains(&key(**p))).count();
            if remaining >= 3 {
                ring.retain(|p| !drop.contains(&key(*p)));
            }
        }
    }
}

/// The result of one operation: meshes for the new groups, in order, each
/// with the index of the input whose layer it takes.
pub struct SolidResult {
    pub meshes: Vec<(Mesh, usize)>,
    /// Indices of the inputs that leave the model.
    pub removed: Vec<usize>,
}

/// Run `op` on world-space solid inputs (in pick order: for Subtract and
/// Trim the first is the cutter, the second the target). Inputs must
/// already be checked as solids.
pub fn run(op: SolidOp, inputs: &[SolidInput]) -> Result<SolidResult, SolidError> {
    if inputs.len() < 2 {
        return Err(SolidError("two solids".into()));
    }
    let mut sources = Sources::default();
    let mut mans = Vec::new();
    for i in inputs {
        mans.push(to_manifold(i, &mut sources)?);
    }
    let out = |man: &Manifold, drop_voids: bool| -> Option<Mesh> {
        let loops = loops_from(man, &sources, drop_voids);
        (!loops.is_empty()).then(|| mesh_from_loops(loops, &sources))
    };
    let all: Vec<usize> = (0..inputs.len()).collect();
    let target = if matches!(op, SolidOp::Subtract | SolidOp::Trim) {
        1
    } else {
        0
    };
    match op {
        SolidOp::Union | SolidOp::MergeSolids => {
            let man = Manifold::batch_boolean(&mans, OpType::Add);
            let m =
                out(&man, op == SolidOp::MergeSolids).ok_or_else(|| SolidError("empty".into()))?;
            Ok(SolidResult {
                meshes: vec![(m, target)],
                removed: all,
            })
        }
        SolidOp::Intersect => {
            let man = Manifold::batch_boolean(&mans, OpType::Intersect);
            let m = out(&man, false).ok_or_else(|| SolidError("no overlap".into()))?;
            Ok(SolidResult {
                meshes: vec![(m, target)],
                removed: all,
            })
        }
        _ => {
            let (a, b) = (&mans[0], &mans[1]);
            if a.intersection(b).is_empty() {
                return Err(SolidError("no overlap".into()));
            }
            match op {
                SolidOp::Subtract | SolidOp::Trim => {
                    let m =
                        out(&b.difference(a), false).ok_or_else(|| SolidError("empty".into()))?;
                    let removed = if op == SolidOp::Subtract {
                        vec![0, 1]
                    } else {
                        vec![1]
                    };
                    Ok(SolidResult {
                        meshes: vec![(m, target)],
                        removed,
                    })
                }
                _ => {
                    let mut meshes = Vec::new();
                    if let Some(m) = out(&b.difference(a), false) {
                        meshes.push((m, 1));
                    }
                    if let Some(m) = out(&a.difference(b), false) {
                        meshes.push((m, 0));
                    }
                    if let Some(m) = out(&a.intersection(b), false) {
                        meshes.push((m, 0));
                    }
                    Ok(SolidResult {
                        meshes,
                        removed: vec![0, 1],
                    })
                }
            }
        }
    }
}
