// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! World-space line batches the GPU pass draws with the depth test on.

use glam::DVec3;
use xtrude_model::guide::Guide;
use xtrude_model::ids::ObjId;

/// One guide's run of dash vertices: `(first_vertex, count, selected)`.
pub type Span = (usize, usize, bool);

/// The most dashes one guide line gets, however fine the spacing.
pub const MAX_DASHES: usize = 4000;

/// Guides as world-space DASHES, so the geometry in front of them hides
/// them like any other line. A guide line is half a million
/// metres long, so the run is centred on its closest point to `centre`
/// (the camera target) and reaches `reach` metres each way (500 spacings
/// when `reach` ≤ 0), `spacing` scaled to the camera distance by the
/// caller. A guide point measured from a vertex draws its finite guide
/// segment; bare guide points draw nothing here (they are screen-space
/// crosses in the overlay).
///
/// Returns the vertices, two per dash, and one span per guide drawn.
pub fn guide_vertices(
    guides: &[Guide],
    spacing: f64,
    selection: &[ObjId],
    centre: Option<DVec3>,
    reach: f64,
) -> (Vec<DVec3>, Vec<Span>) {
    let mut coords: Vec<DVec3> = Vec::new();
    let mut spans = Vec::new();
    let spacing = spacing.max(1e-6);
    let reach = if reach > 0.0 { reach } else { spacing * 500.0 };
    let dash = spacing * 0.5;
    let origin = centre.unwrap_or(DVec3::ZERO);
    let n = ((2.0 * reach / spacing) as usize).clamp(1, MAX_DASHES);
    for g in guides {
        let start = coords.len();
        match g.direction {
            None => {
                let Some(o) = g.origin else { continue };
                let seg = g.point - o;
                let length = seg.length();
                if length < 1e-9 {
                    continue;
                }
                let d = seg / length;
                let mut t = 0.0;
                while t < length {
                    coords.push(o + d * t);
                    coords.push(o + d * (t + dash).min(length));
                    t += spacing;
                }
            }
            Some(d) => {
                // Foot of the perpendicular from the centre: the stretch
                // of guide the camera is looking at.
                let t0 = (origin - g.point).dot(d) - reach;
                for k in 0..n {
                    let t = t0 + k as f64 * spacing;
                    coords.push(g.point + d * t);
                    coords.push(g.point + d * (t + dash));
                }
            }
        }
        let count = coords.len() - start;
        if count > 0 {
            spans.push((start, count, selection.contains(&g.id)));
        }
    }
    (coords, spans)
}

/// The twelve edges of a box (`corners` in `oriented_box_corners` order)
/// as world-space dashes, segment end points in pairs: the part behind the
/// geometry is hidden like any line, the way guides are.
pub fn box_dash_vertices(corners: &[DVec3], spacing: f64) -> Vec<DVec3> {
    let mut out = Vec::new();
    let spacing = spacing.max(1e-4);
    let dash = spacing * 0.5;
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            let j = i | bit;
            if j == i {
                continue;
            }
            let (a, b) = (corners[i], corners[j]);
            let len = (b - a).length();
            if len < 1e-9 {
                continue;
            }
            let u = (b - a) / len;
            let mut t = 0.0;
            while t < len {
                out.push(a + u * t);
                out.push(a + u * (t + dash).min(len));
                t += spacing;
            }
        }
    }
    out
}
