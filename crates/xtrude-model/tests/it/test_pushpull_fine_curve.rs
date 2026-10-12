// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A push on a solid with a fine curve: facets a degree and a half apart
//! and a few millimetres wide, each within 0.1 mm of its neighbour's plane.
//! The plane rebuild flattens such a facet onto its neighbour and cracks
//! the next seam; the push keeps the stitched solid instead, with the new
//! walls merged into the faces they continue, and commits exactly.

use std::f64::consts::FRAC_PI_2;

use xtrude_core::math::{newell, VecExt};
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::topology::{is_planar, PLANAR_TOLERANCE};
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::pushpull::{push_face_in_scene, PushOutcome};
use xtrude_model::scene::Scene;

use crate::common;

/// The quarter round's radius and facets: 1.4° apart, 2.45 mm wide.
const R: f64 = 0.1;
const FACETS: usize = 64;
/// The block: x from 0 to `L`, y from 0 to `W`, z from 0 to `H` (at
/// x = 0; `rise` more at x = `L`), the edge y = `W`, z = 0 rounded.
const L: f64 = 0.2;
const W: f64 = 0.15;
const H: f64 = 0.15;

/// The profile in (y, z), counter-clockwise seen from +x: the rounded
/// corner's arc from the bottom to the back.
fn profile(top: f64) -> Vec<(f64, f64)> {
    let mut p = vec![(0.0, 0.0), (W - R, 0.0)];
    for i in 1..FACETS {
        let a = -FRAC_PI_2 + FRAC_PI_2 * i as f64 / FACETS as f64;
        p.push((W - R + R * a.cos(), R + R * a.sin()));
    }
    p.extend([(W, R), (W, top), (0.0, top)]);
    p
}

/// The block as a closed solid: the end faces at x = 0 and x = `L`, one
/// face per side of the profile between them.
fn block(rise: f64) -> Scene {
    let (mut s, _) = common::scene_history();
    let (p0, p1) = (profile(H), profile(H + rise));
    let at = |x: f64, (y, z): (f64, f64)| DVec3::new(x, y, z);
    let m: &mut Mesh = s.mesh_mut();
    m.add_face(
        &p0.iter().rev().map(|&q| at(0.0, q)).collect::<Vec<_>>(),
        &[],
    );
    m.add_face(&p1.iter().map(|&q| at(L, q)).collect::<Vec<_>>(), &[]);
    let n = p0.len();
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_face(
            &[at(0.0, p0[i]), at(0.0, p0[j]), at(L, p1[j]), at(L, p1[i])],
            &[],
        );
    }
    orient_outward(m, DEFAULT_SEED, None);
    assert!(is_closed(m));
    s
}

fn end_face(s: &Scene, x: f64) -> FaceId {
    common::find_face(s, |p| {
        p.len() > 4 && p.iter().all(|q| (q.x - x).abs() < 1e-12)
    })
    .expect("the end face")
}

fn area(m: &Mesh, f: FaceId) -> f64 {
    newell(&m.face_positions(f)).length() / 2.0
}

/// Push `face` by `d`: committed exactly, closed, every face flat, the
/// volume grown by the face's area times `d`. Returns the face count.
fn pushes_exactly(s: &mut Scene, face: FaceId, d: f64) -> usize {
    let before = s.mesh().clone();
    let a = area(&before, face);
    let mid = s.active_mesh_id();
    let pp = push_face_in_scene(s, mid, face, d, false)
        .expect("the push runs")
        .expect("the push happens");
    assert_eq!(pp.outcome(), PushOutcome::Committed, "push by {d}");
    assert_eq!(pp.extrusion, d);
    let m = s.mesh();
    assert!(is_closed(m), "push by {d}");
    assert!(m
        .face_ids()
        .into_iter()
        .all(|f| is_planar(&m.face_positions(f), PLANAR_TOLERANCE)));
    let grown = signed_volume(m) - signed_volume(&before);
    assert!((grown - a * d).abs() < 1e-12, "{grown} vs {}", a * d);
    m.faces.len()
}

#[test]
fn test_the_end_of_a_block_with_a_fine_curve_pushes_both_ways() {
    for x in [0.0, L] {
        for d in [0.01, -0.019] {
            let mut s = block(0.0);
            let f = end_face(&s, x);
            let faces = s.mesh().faces.len();
            // A prism: the end moves, nothing is added.
            assert_eq!(pushes_exactly(&mut s, f, d), faces, "x = {x}, d = {d}");
        }
    }
}

#[test]
fn test_new_walls_along_a_fine_curve_merge_into_the_faces_they_continue() {
    // The top slopes down to x = L: that end is no prism, the push makes
    // walls (the top must not hang over it, or it would not go in).
    for d in [0.01, -0.019] {
        let mut s = block(-0.02);
        let f = end_face(&s, L);
        let faces = s.mesh().faces.len();
        // One wall, under the slanting top; the rest continue a face.
        assert_eq!(pushes_exactly(&mut s, f, d), faces + 1, "d = {d}");
        // No edge left between two faces of one plane.
        let m = s.mesh();
        let flat = m.edge_ids().into_iter().filter(|&e| {
            let ef = &m.edge(e).faces;
            ef.len() == 2
                && m.face_normal(ef[0])
                    .unit_or_zero()
                    .dot(m.face_normal(ef[1]).unit_or_zero())
                    > 1.0 - 1e-9
        });
        assert_eq!(flat.count(), 0, "d = {d}");
    }
}
