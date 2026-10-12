// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Areas and volumes of face sets.
//!
//! Any face set yields its AREA; VOLUME is reported only when the set is
//! watertight on its own (every edge of the set bordered by exactly two
//! faces of the set).

use std::collections::HashMap;

use crate::math::newell;
use crate::mesh::{FaceId, Mesh, VertexId};

/// Face area with hole loops (openings) subtracted.
pub fn face_net_area(mesh: &Mesh, f: FaceId) -> f64 {
    let mut area = mesh.face_area(f);
    for h in mesh.face_hole_positions(f) {
        area -= 0.5 * newell(&h).length();
    }
    area.max(0.0)
}

/// Volume enclosed by `faces`, or `None` when the set is not watertight by
/// itself (signed tetrahedron sum over the set's triangles).
pub fn face_set_volume(mesh: &Mesh, faces: &[FaceId]) -> Option<f64> {
    if faces.is_empty() {
        return None;
    }
    let mut counts: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for &f in faces {
        for lp in mesh.face(f).loops() {
            let n = lp.len();
            for i in 0..n {
                let (a, b) = (lp[i], lp[(i + 1) % n]);
                let k = if a <= b { (a, b) } else { (b, a) };
                *counts.entry(k).or_insert(0) += 1;
            }
        }
    }
    if counts.values().any(|&c| c != 2) {
        return None;
    }
    let mut total = 0.0;
    for &f in faces {
        for [a, b, c] in mesh.face_triangulate(f, None) {
            total += a.dot(b.cross(c)) / 6.0;
        }
    }
    Some(total.abs())
}
