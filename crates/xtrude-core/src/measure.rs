// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Areas, volumes and mass properties of face sets.
//!
//! Any face set yields its AREA; VOLUME and the MASS PROPERTIES are
//! reported only when the set is watertight on its own (every edge of the
//! set bordered by exactly two faces of the set).

use std::collections::HashMap;

use crate::math::{newell, DMat3, DVec3};
use crate::mesh::{FaceId, Mesh, VertexId};

/// Face area with hole loops (openings) subtracted.
pub fn face_net_area(mesh: &Mesh, f: FaceId) -> f64 {
    let mut area = mesh.face_area(f);
    for h in mesh.face_hole_positions(f) {
        area -= 0.5 * newell(&h).length();
    }
    area.max(0.0)
}

/// Whether every edge of `faces` borders exactly two faces of the set.
fn is_closed_set(mesh: &Mesh, faces: &[FaceId]) -> bool {
    if faces.is_empty() {
        return false;
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
    counts.values().all(|&c| c == 2)
}

/// Volume enclosed by `faces`, or `None` when the set is not watertight by
/// itself (signed tetrahedron sum over the set's triangles).
pub fn face_set_volume(mesh: &Mesh, faces: &[FaceId]) -> Option<f64> {
    if !is_closed_set(mesh, faces) {
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

/// The mass properties of a closed face set, per unit density: in kg for a
/// density of 1 kg/m³ when the mesh is in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    /// The enclosed volume (positive whichever way the faces point).
    pub volume: f64,
    /// The centre of mass (the centroid of the volume).
    pub centre: DVec3,
    /// The inertia tensor about `centre`, along the mesh's axes.
    pub inertia: DMat3,
}

/// The volume, centre of mass and inertia tensor about it of the solid
/// `faces` enclose, per unit density, or `None` when the set is not
/// watertight by itself or encloses no volume.
///
/// Divergence theorem: each triangle `(a, b, c)` and a fixed point span a
/// signed tetrahedron of volume `d / 6`, `d = a · (b × c)`, whose first
/// moment is `d / 24 · (a + b + c)` and whose second moment `∫ x xᵀ dV` is
/// `d / 120 · (a aᵀ + b bᵀ + c cᵀ + s sᵀ)`, `s = a + b + c`. Their sums are
/// the solid's (positions taken from the fixed point, a vertex of the
/// set); faces pointing inwards flip every sign, which the volume's sign
/// undoes.
pub fn mass_properties(mesh: &Mesh, faces: &[FaceId]) -> Option<MassProperties> {
    if !is_closed_set(mesh, faces) {
        return None;
    }
    let outer = |u: DVec3| DMat3::from_cols(u * u.x, u * u.y, u * u.z);
    // Sums about a point of the set rather than the origin: far from the
    // origin, the second moments would cancel and lose the inertia.
    let origin = mesh.face_positions(faces[0])[0];
    let mut volume = 0.0;
    let mut first = DVec3::ZERO;
    let mut second = DMat3::ZERO;
    for &f in faces {
        for [a, b, c] in mesh.face_triangulate(f, None) {
            let (a, b, c) = (a - origin, b - origin, c - origin);
            let d = a.dot(b.cross(c));
            let s = a + b + c;
            volume += d / 6.0;
            first += s * (d / 24.0);
            second += (outer(a) + outer(b) + outer(c) + outer(s)) * (d / 120.0);
        }
    }
    if volume == 0.0 || !volume.is_finite() {
        return None;
    }
    let sign = volume.signum();
    let (volume, first, second) = (volume * sign, first * sign, second * sign);
    let centre = first / volume;
    // The second moment about the centre, then I = tr(S) 1 − S.
    let about = second - outer(centre) * volume;
    let centre = centre + origin;
    let trace = about.x_axis.x + about.y_axis.y + about.z_axis.z;
    let inertia = DMat3::from_diagonal(DVec3::splat(trace)) - about;
    Some(MassProperties {
        volume,
        centre,
        inertia,
    })
}
