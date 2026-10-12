// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Exploded view: a component's parts pulled apart, and put back.
//!
//! Each part moves away from the assembly's centre in proportion to how far
//! it already sits from it — in every direction (`outward`) or along one
//! axis only (`x`, `y`, `z`). Each part remembers the translation the
//! explosion gave it (`explode_offset`) and the container the setting
//! (`exploded`), so Reassemble takes only the explosion's share off.

use xtrude_core::math::{DMat4, DVec3, MatExt};

use crate::group::{Exploded, Group, MeshTable};
use crate::parts::part_points;

pub const MODES: [&str; 4] = ["outward", "x", "y", "z"];

fn offset_of(part: &Group) -> DVec3 {
    part.explode_offset.unwrap_or(DVec3::ZERO)
}

/// Each part's centre where it sits assembled (container frame).
pub fn assembled_centres(meshes: &MeshTable, container: &Group) -> Vec<DVec3> {
    container
        .children
        .iter()
        .map(|kid| {
            let pts = part_points(meshes, kid);
            let c = if pts.is_empty() {
                DVec3::ZERO
            } else {
                let lo = pts
                    .iter()
                    .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
                let hi = pts
                    .iter()
                    .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
                (lo + hi) / 2.0
            };
            c - offset_of(kid)
        })
        .collect()
}

/// The translation each part takes at `factor`.
pub fn explode_offsets(centres: &[DVec3], factor: f64, mode: &str) -> Vec<DVec3> {
    if centres.is_empty() {
        return Vec::new();
    }
    let lo = centres
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
    let hi = centres
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
    let mid = (lo + hi) / 2.0;
    let keep = match mode {
        "x" => DVec3::X,
        "y" => DVec3::Y,
        "z" => DVec3::Z,
        _ => DVec3::ONE,
    };
    centres.iter().map(|&c| (c - mid) * factor * keep).collect()
}

/// Pull `container`'s parts apart to `factor` (`0` reassembles), in place.
pub fn apply_explode(
    meshes: &MeshTable,
    container: &mut Group,
    factor: f64,
    mode: &str,
    centres: Option<Vec<DVec3>>,
) {
    if container.children.is_empty() {
        return;
    }
    let centres = centres.unwrap_or_else(|| assembled_centres(meshes, container));
    let factor = factor.max(0.0);
    let new: Vec<Option<DVec3>> = if factor > 0.0 {
        explode_offsets(&centres, factor, mode)
            .into_iter()
            .map(Some)
            .collect()
    } else {
        vec![None; container.children.len()]
    };
    for (kid, target) in container.children.iter_mut().zip(new) {
        let old = offset_of(kid);
        let step = match target {
            Some(t) => t - old,
            None => -old,
        };
        kid.xform = Some(DMat4::from_translation(step) * kid.xform.unwrap_or(DMat4::IDENTITY));
        kid.explode_offset = target.filter(|t| *t != DVec3::ZERO);
    }
    container.exploded = (factor > 0.0).then(|| Exploded {
        factor,
        mode: mode.to_string(),
    });
}

/// Re-express the parts' offsets after `xform` was pushed down into them.
pub fn rotate_offsets(container: &mut Group, xform: &DMat4) {
    for kid in container.children.iter_mut() {
        if let Some(o) = kid.explode_offset {
            kid.explode_offset = Some(xform.transform_vector(o));
        }
    }
}
