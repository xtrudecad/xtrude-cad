// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Loops of a few mm² are faces and holes like any other; only degenerate
//! loops, whose area is within the squared length tolerance, are dropped.

use xtrude_core::arrangement::planar_arrangement;
use xtrude_core::cap_rebuild::{rebuild_plane, RebuildOpts, RebuiltFace};
use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::topology::heal_overlapping_faces;
use xtrude_core::{DVec3, Mesh};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// The closed loop through `pts` on the ground, as segments.
fn ring(pts: &[(f64, f64)]) -> Vec<(DVec3, DVec3)> {
    (0..pts.len())
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            (v(p.0, p.1, 0.0), v(q.0, q.1, 0.0))
        })
        .collect()
}

/// A square of side `s` with its low corner at `(x, y)`.
fn square(x: f64, y: f64, s: f64) -> Vec<(f64, f64)> {
    vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)]
}

/// A triangle 0.14 mm on its legs: its corners snap to 0.1 mm apart, so
/// its area (0.005 mm²) is below the squared length tolerance.
const SLIVER: [(f64, f64); 3] = [(0.0, 0.0), (0.00014, 0.0), (0.0, 0.00014)];

#[test]
fn test_arrangement_keeps_a_2_mm_square() {
    let faces = planar_arrangement(&ring(&square(0.0, 0.0, 0.002)), DVec3::ZERO, DVec3::Z);
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].0.len(), 4);
}

#[test]
fn test_arrangement_nests_a_2_mm_square_as_a_hole() {
    let mut segs = ring(&square(0.0, 0.0, 0.010));
    segs.extend(ring(&square(0.004, 0.004, 0.002)));
    let faces = planar_arrangement(&segs, DVec3::ZERO, DVec3::Z);
    assert_eq!(faces.len(), 2);
    let holes: usize = faces.iter().map(|(_, h)| h.len()).sum();
    assert_eq!(holes, 1);
}

#[test]
fn test_arrangement_drops_degenerate_loops() {
    let faces = planar_arrangement(&ring(&SLIVER), DVec3::ZERO, DVec3::Z);
    assert!(faces.is_empty());
    let flat = ring(&[(0.0, 0.0), (0.001, 0.0), (0.002, 0.0)]);
    assert!(planar_arrangement(&flat, DVec3::ZERO, DVec3::Z).is_empty());
}

/// A closed prism of height `h` over the footprint `pts`.
fn prism(m: &mut Mesh, pts: &[(f64, f64)], h: f64) {
    let b: Vec<DVec3> = pts.iter().map(|&(x, y)| v(x, y, 0.)).collect();
    let t: Vec<DVec3> = pts.iter().map(|&(x, y)| v(x, y, h)).collect();
    m.add_face(&b, &[]);
    m.add_face(&t, &[]);
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        m.add_face(&[b[i], b[j], t[j], t[i]], &[]);
    }
}

fn rebuild_top(m: &Mesh, h: f64) -> Vec<RebuiltFace> {
    rebuild_plane(
        m,
        v(0., 0., h),
        DVec3::Z,
        &[],
        RebuildOpts::default(),
        None,
        None,
    )
    .unwrap_or_default()
}

#[test]
fn test_cap_rebuild_keeps_the_top_of_a_2_mm_cube() {
    let mut m = Mesh::new();
    prism(&mut m, &square(0.0, 0.0, 0.002), 0.002);
    orient_outward(&mut m, DEFAULT_SEED, None);
    let faces = rebuild_top(&m, 0.002);
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].0.len(), 4);
}

#[test]
fn test_cap_rebuild_drops_a_degenerate_cap() {
    let mut m = Mesh::new();
    prism(&mut m, &square(0.010, 0.0, 0.002), 0.002);
    prism(&mut m, &SLIVER, 0.002);
    orient_outward(&mut m, DEFAULT_SEED, None);
    let faces = rebuild_top(&m, 0.002);
    assert_eq!(faces.len(), 1, "only the cube's top");
    assert_eq!(faces[0].0.len(), 4);
}

#[test]
fn test_heal_removes_a_mother_of_a_few_mm() {
    let g = |x: f64, y: f64| v(x, y, 0.0);
    let mut m = Mesh::new();
    m.add_face(
        &[g(0., 0.), g(0.008, 0.), g(0.008, 0.008), g(0., 0.008)],
        &[],
    );
    m.add_face(
        &[
            g(0.0002, 0.0002),
            g(0.0038, 0.0002),
            g(0.0038, 0.0078),
            g(0.0002, 0.0078),
        ],
        &[],
    );
    m.add_face(
        &[
            g(0.0042, 0.0002),
            g(0.0078, 0.0002),
            g(0.0078, 0.0078),
            g(0.0042, 0.0078),
        ],
        &[],
    );
    assert_eq!(heal_overlapping_faces(&mut m, 0.5, None, None).len(), 1);
    assert_eq!(m.faces.len(), 2);
}
