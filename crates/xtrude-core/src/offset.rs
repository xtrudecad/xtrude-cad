// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Robust polygon offset: slide the edges, let the arrangement sort out the
//! mess, keep the regions that are really there.
//!
//! 1. Slide each edge by `d`.
//! 2. Join the corners with a MITRE, bevelling past the mitre limit.
//! 3. Hand the segments to the arrangement, which resolves the crossings.
//! 4. Keep the regions whose interior really sits `d` inside the original.

use crate::arrangement::{planar_arrangement, plane_basis, point_in_polygon, Face3};
use crate::math::{DVec3, VecExt};
pub use crate::topology::interior_point;
use crate::topology::offset_line_intersection;

/// How far past the offset corner a mitre may reach before it is bevelled,
/// as a multiple of the distance (Clipper's default).
pub const MITRE_LIMIT: f64 = 2.0;
const KEEP: f64 = 0.75;

fn dist_to_boundary(p: DVec3, lp: &[DVec3]) -> f64 {
    let mut best = f64::INFINITY;
    let n = lp.len();
    for i in 0..n {
        let a = lp[i];
        let ab = lp[(i + 1) % n] - a;
        let l2 = ab.length_squared();
        if l2 < 1e-18 {
            best = best.min((p - a).length());
            continue;
        }
        let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
        best = best.min((p - (a + ab * t)).length());
    }
    best
}

/// Whether `p` lies inside `lp` (in its plane).
pub fn inside(p: DVec3, lp: &[DVec3], normal: DVec3) -> bool {
    let (u, w) = plane_basis(normal);
    let o = lp[0];
    let proj = |q: DVec3| ((q - o).dot(u), (q - o).dot(w));
    point_in_polygon(proj(p), &lp.iter().map(|&q| proj(q)).collect::<Vec<_>>())
}

fn offset_segments(lp: &[DVec3], normal: DVec3, d: f64) -> Vec<(DVec3, DVec3)> {
    let mut lines: Vec<(DVec3, DVec3, DVec3)> = Vec::new();
    let count = lp.len();
    for i in 0..count {
        let a = lp[i];
        let b = lp[(i + 1) % count];
        let edge = b - a;
        if edge.length() < 1e-9 {
            continue;
        }
        let dir = edge.unit_or_zero();
        let push = normal.cross(dir).unit_or_zero() * d;
        lines.push((a + push, b + push, dir));
    }
    let total = lines.len();
    if total < 3 {
        return Vec::new();
    }
    let reach = MITRE_LIMIT * d.abs() + 1e-9;
    let corners: Vec<Option<DVec3>> = (0..total)
        .map(|i| {
            let (_, end, dir) = lines[i];
            let nxt = lines[(i + 1) % total];
            offset_line_intersection(end, dir, nxt.0, nxt.2, normal)
                .filter(|c| (*c - end).length() <= reach)
        })
        .collect();
    let mut segs = Vec::new();
    for i in 0..total {
        let (start, end, _) = lines[i];
        let back = corners[(i + total - 1) % total];
        let here = corners[i];
        segs.push((back.unwrap_or(start), here.unwrap_or(end)));
        if here.is_none() {
            segs.push((end, lines[(i + 1) % total].0));
        }
    }
    segs
}

/// Offset `lp` by `d` (positive inward) and return the regions that survive.
pub fn offset_regions(lp: &[DVec3], normal: DVec3, d: f64) -> Vec<Face3> {
    if lp.len() < 3 || d.abs() < 1e-9 {
        return Vec::new();
    }
    let unit = normal.unit_or_zero();
    if unit.length_squared() < 0.5 {
        return Vec::new();
    }
    let segs = offset_segments(lp, unit, d);
    if segs.is_empty() {
        return Vec::new();
    }
    let faces = planar_arrangement(&segs, lp[0], unit);
    let mut kept = Vec::new();
    for (outer, holes) in faces {
        let Some(point) = interior_point(&outer, unit) else {
            continue;
        };
        let mut depth = dist_to_boundary(point, lp);
        if !inside(point, lp, unit) {
            depth = -depth;
        }
        let reject = if d > 0.0 { depth < d * KEEP } else { depth < d };
        if reject {
            continue;
        }
        kept.push((outer, holes));
    }
    kept
}

/// Offset an OPEN polyline by `d` along `cross(normal, direction)`.
pub fn offset_chain(points: &[DVec3], normal: DVec3, d: f64) -> Option<Vec<DVec3>> {
    let n = normal.unit_or_zero();
    let mut lines: Vec<(DVec3, DVec3, DVec3)> = Vec::new();
    for w in points.windows(2) {
        let edge = w[1] - w[0];
        if edge.length() < 1e-9 {
            continue;
        }
        let dir = edge.unit_or_zero();
        let push = n.cross(dir).unit_or_zero() * d;
        lines.push((w[0] + push, w[1] + push, dir));
    }
    if lines.is_empty() {
        return None;
    }
    let reach = MITRE_LIMIT * d.abs() + 1e-9;
    let mut out = vec![lines[0].0];
    for i in 0..lines.len() - 1 {
        let (_, end, dir) = lines[i];
        let nxt = lines[i + 1];
        match offset_line_intersection(end, dir, nxt.0, nxt.2, n)
            .filter(|c| (*c - end).length() <= reach)
        {
            Some(c) => out.push(c),
            None => {
                out.push(end);
                out.push(nxt.0);
            }
        }
    }
    out.push(lines[lines.len() - 1].1);
    (out.len() >= 2).then_some(out)
}
