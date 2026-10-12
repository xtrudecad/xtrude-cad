// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The watertight stitch pass: weld,
//! T-junctions, collinear vertices, and the seeded coplanar merge.

use std::collections::{HashMap, HashSet};

use crate::math::{DVec3, VecExt};
use crate::mesh::{FaceId, Key, Mesh};
use crate::topology::{sweep_tjunctions, tkey};

/// The seed bounding box of a set of position keys (topology keys are
/// positions rounded to the weld tolerance).
pub fn seed_bounds(seed: &[DVec3]) -> Option<(DVec3, DVec3)> {
    if seed.is_empty() {
        return None;
    }
    let keys: Vec<DVec3> = seed
        .iter()
        .map(|&p| {
            let k = tkey(p);
            DVec3::new(k.0 as f64 * 1e-4, k.1 as f64 * 1e-4, k.2 as f64 * 1e-4)
        })
        .collect();
    let lo = keys
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
    let hi = keys
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
    Some((lo, hi))
}

/// Three-phase watertight cleanup (the caller snapshots for undo).
///
/// 0. weld coincident vertices (and dedupe identical cycles);
/// 1. resolve T-junctions, scoped to the operation's box;
/// 2. collapse redundant valence-2 collinear vertices;
/// 3. coplanar-merge the components that hold a face the op created.
pub fn run_stitch(
    mesh: &mut Mesh,
    seed: &[DVec3],
    mut new_faces: Option<&mut HashSet<FaceId>>,
    coplanar_merge: bool,
    dedupe: bool,
) {
    let seedkeys: HashSet<Key> = seed.iter().map(|&p| tkey(p)).collect();
    mesh.weld_coincident();
    if dedupe {
        mesh.dedupe_faces();
    }
    sweep_tjunctions(mesh, seed_bounds(seed));
    loop {
        let mut collapsed = false;
        for v in mesh.vertex_ids() {
            if mesh.has_vertex(v) && mesh.collapsible_vertex(v) {
                mesh.collapse_vertex(v);
                collapsed = true;
                break;
            }
        }
        if !collapsed {
            break;
        }
    }
    if !coplanar_merge {
        return;
    }
    loop {
        let mut merged = false;
        let mut ncache: HashMap<FaceId, DVec3> = HashMap::new();
        let mut scache: HashMap<FaceId, bool> = HashMap::new();
        for f0 in mesh.face_ids() {
            if !mesh.has_face(f0) {
                continue;
            }
            let comp = coplanar_component(mesh, f0, &seedkeys, &mut ncache, &mut scache);
            if comp.len() < 2 {
                continue;
            }
            if let Some(nf) = new_faces.as_deref() {
                if !comp.iter().any(|f| nf.contains(f)) {
                    continue;
                }
            }
            if let Some(region) = mesh.dissolve_coplanar_region(&comp) {
                if let Some(nf) = new_faces.as_deref_mut() {
                    for f in &comp {
                        nf.remove(f);
                    }
                    nf.insert(region);
                }
                merged = true;
                break;
            }
        }
        if !merged {
            break;
        }
    }
}

/// Maximal set of coplanar, edge-connected faces that touch the seed,
/// never crossing a crease (an edge that also carries a non-coplanar face).
pub fn coplanar_component(
    mesh: &Mesh,
    f0: FaceId,
    seedkeys: &HashSet<Key>,
    ncache: &mut HashMap<FaceId, DVec3>,
    scache: &mut HashMap<FaceId, bool>,
) -> Vec<FaceId> {
    let mut nrm = |f: FaceId| {
        *ncache
            .entry(f)
            .or_insert_with(|| mesh.face_normal(f).unit_or_zero())
    };
    let mut seeded = |f: FaceId| {
        *scache.entry(f).or_insert_with(|| {
            mesh.face_positions(f)
                .iter()
                .any(|&p| seedkeys.contains(&tkey(p)))
        })
    };
    if !seeded(f0) {
        return Vec::new();
    }
    let n0 = nrm(f0);
    let mut comp: indexmap::IndexSet<FaceId> = indexmap::IndexSet::new();
    comp.insert(f0);
    let mut stack = vec![f0];
    while let Some(f) = stack.pop() {
        let loops: Vec<Vec<_>> = mesh.face(f).loops().cloned().collect();
        for lp in loops {
            let n = lp.len();
            for i in 0..n {
                let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) else {
                    continue;
                };
                let ef = mesh.edge(e).faces.clone();
                if ef.iter().any(|&h| n0.dot(nrm(h)).abs() < 0.999) {
                    continue; // crease
                }
                for g in ef {
                    if comp.contains(&g) || !seeded(g) {
                        continue;
                    }
                    if n0.dot(mesh.face_normal(g)).abs() > 0.999 {
                        comp.insert(g);
                        stack.push(g);
                    }
                }
            }
        }
    }
    comp.into_iter().collect()
}
