// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The parts of a component, measured the way a cut list measures them:
//! name, the material most of it wears, and length × width × thickness of
//! the box a woodworker would cut it from.

use indexmap::IndexMap;
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::math::{symmetric_eigenvectors, DVec3, MatExt};

use crate::group::{iter_placements, Group, MeshTable};
use crate::materials::has_own_material;

/// `part`'s vertex positions in its container's coordinates.
pub fn part_points(meshes: &MeshTable, part: &Group) -> Vec<DVec3> {
    let mut out = Vec::new();
    for (g, m) in iter_placements(part, None) {
        let Some(mesh) = meshes.get(&g.mesh) else {
            continue;
        };
        for (_, v) in mesh.verts.iter() {
            out.push(m.map_or(v.position, |mm| mm.project_point(v.position)));
        }
    }
    out
}

/// How much smaller the part's own box must be to beat the model's axes.
const OWN_AXES_GAIN: f64 = 0.8;

fn extent(points: &[DVec3], axes: [DVec3; 3]) -> DVec3 {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in points {
        let q = DVec3::new(p.dot(axes[0]), p.dot(axes[1]), p.dot(axes[2]));
        lo = lo.min(q);
        hi = hi.max(q);
    }
    hi - lo
}

/// `(length, width, thickness)`, largest first: the tighter of the box
/// along the container's axes and the box along the part's own principal
/// axes.
pub fn part_size(points: &[DVec3]) -> (f64, f64, f64) {
    if points.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut best = extent(points, [DVec3::X, DVec3::Y, DVec3::Z]);
    if points.len() >= 4 {
        let mean = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        let mut cov = [[0.0f64; 3]; 3];
        for p in points {
            let c = *p - mean;
            let v = [c.x, c.y, c.z];
            for i in 0..3 {
                for j in 0..3 {
                    cov[i][j] += v[i] * v[j];
                }
            }
        }
        let vecs = symmetric_eigenvectors(cov);
        let own = extent(points, vecs);
        if own.x * own.y * own.z < OWN_AXES_GAIN * best.x * best.y * best.z {
            best = own;
        }
    }
    let mut dims = [best.x, best.y, best.z];
    dims.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    (dims[0], dims[1], dims[2])
}

fn material_label(attrs: &FaceAttrs) -> Option<String> {
    if let Some(m) = &attrs.paint.mat {
        if !m.is_empty() {
            return Some(m.clone());
        }
    }
    attrs.paint.color.map(|c| {
        let to = |x: f64| xtrude_core::math::round_even_int(x * 255.0);
        format!("#{:02x}{:02x}{:02x}", to(c[0]), to(c[1]), to(c[2]))
    })
}

/// The material covering most of `part`'s surface ('' when none).
pub fn part_material(meshes: &MeshTable, part: &Group) -> String {
    let mut area: IndexMap<String, f64> = IndexMap::new();
    for (g, _) in iter_placements(part, None) {
        let inherited = g.material.clone().or_else(|| part.material.clone());
        let Some(mesh) = meshes.get(&g.mesh) else {
            continue;
        };
        for (fid, f) in mesh.faces.iter() {
            let label = if has_own_material(&f.attrs) {
                material_label(&f.attrs)
            } else {
                inherited.as_ref().and_then(|p| {
                    material_label(&FaceAttrs {
                        paint: p.clone(),
                        ..FaceAttrs::default()
                    })
                })
            };
            if let Some(l) = label {
                *area.entry(l).or_insert(0.0) += mesh.face_area(fid);
            }
        }
    }
    area.into_iter()
        .fold(None::<(String, f64)>, |best, (k, v)| match best {
            Some((bk, bv)) if bv >= v => Some((bk, bv)),
            _ => Some((k, v)),
        })
        .map(|(k, _)| k)
        .unwrap_or_default()
}

/// One part of a container, measured like a cut list.
#[derive(Clone, Debug)]
pub struct PartRow {
    pub part: crate::ids::GroupId,
    pub name: String,
    pub material: String,
    pub size: (f64, f64, f64),
    pub faces: usize,
    pub hidden: bool,
}

pub fn part_rows(meshes: &MeshTable, container: &Group) -> Vec<PartRow> {
    container
        .children
        .iter()
        .map(|child| PartRow {
            part: child.id,
            name: child.name.clone(),
            material: part_material(meshes, child),
            size: part_size(&part_points(meshes, child)),
            faces: iter_placements(child, None)
                .iter()
                .map(|(g, _)| meshes.get(&g.mesh).map_or(0, |m| m.faces.len()))
                .sum(),
            hidden: child.hidden,
        })
        .collect()
}

/// Thinner than this, a part is a SURFACE (metres).
pub const SURFACE_BELOW: f64 = 0.0005;

pub fn is_surface(size: (f64, f64, f64)) -> bool {
    size.2 < SURFACE_BELOW
}
