// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Orient Faces: make every face connected to a chosen face wind the same
//! way it does. Crosses only edges shared by exactly TWO faces.

use std::collections::{HashSet, VecDeque};

use crate::mesh::{FaceId, Mesh, VertexId};

fn directed_edges(mesh: &Mesh, face: FaceId, flipped: bool) -> Vec<(VertexId, VertexId)> {
    let mut out = Vec::new();
    for lp in mesh.face(face).loops() {
        let n = lp.len();
        for i in 0..n {
            let (a, b) = (lp[i], lp[(i + 1) % n]);
            out.push(if flipped { (b, a) } else { (a, b) });
        }
    }
    out
}

fn walks(mesh: &Mesh, face: FaceId, a: VertexId, b: VertexId) -> bool {
    directed_edges(mesh, face, false)
        .iter()
        .any(|&(x, y)| x == a && y == b)
}

/// The faces connected to `seed` that must be reversed so they all wind
/// like it (`seed` itself never in the list).
pub fn faces_to_flip(mesh: &Mesh, seed: FaceId) -> Vec<FaceId> {
    let mut flip: indexmap::IndexMap<FaceId, bool> = indexmap::IndexMap::new();
    flip.insert(seed, false);
    let mut todo = VecDeque::from([seed]);
    while let Some(face) = todo.pop_front() {
        let fl = flip[&face];
        for (a, b) in directed_edges(mesh, face, fl) {
            let Some(e) = mesh.find_edge(a, b) else {
                continue;
            };
            let ef = &mesh.edge(e).faces;
            if ef.len() != 2 {
                continue;
            }
            let Some(&other) = ef.iter().find(|&&f| f != face) else {
                continue;
            };
            if flip.contains_key(&other) {
                continue;
            }
            flip.insert(other, walks(mesh, other, a, b));
            todo.push_back(other);
        }
    }
    flip.into_iter()
        .filter(|(_, f)| *f)
        .map(|(k, _)| k)
        .collect()
}

/// How many faces the orientation reached, `seed` included.
pub fn connected_count(mesh: &Mesh, seed: FaceId) -> usize {
    let mut seen: HashSet<FaceId> = HashSet::from([seed]);
    let mut todo = VecDeque::from([seed]);
    while let Some(face) = todo.pop_front() {
        for (a, b) in directed_edges(mesh, face, false) {
            let Some(e) = mesh.find_edge(a, b) else {
                continue;
            };
            let ef = &mesh.edge(e).faces;
            if ef.len() != 2 {
                continue;
            }
            for &o in ef {
                if seen.insert(o) {
                    todo.push_back(o);
                }
            }
        }
    }
    seen.len()
}
