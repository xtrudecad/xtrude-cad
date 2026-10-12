// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Planar arrangement: minimal faces from coplanar segments, and the command
//! that rebuilds a flat mesh.

use crate::common;
use common::*;
use xtrude_core::arrangement::planar_arrangement;
use xtrude_core::DVec3;
use xtrude_model::commands::{AddEdgeCommand, RebuildPlanarFacesCommand};

const O: DVec3 = DVec3::ZERO;
const N: DVec3 = DVec3::Z;

fn lp(pts: &[(f64, f64)]) -> Vec<(DVec3, DVec3)> {
    (0..pts.len())
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            (V(p.0, p.1), V(q.0, q.1))
        })
        .collect()
}

fn area(l: &[DVec3]) -> f64 {
    let n = l.len();
    (0..n)
        .map(|i| l[i].x * l[(i + 1) % n].y - l[(i + 1) % n].x * l[i].y)
        .sum::<f64>()
        .abs()
        * 0.5
}

#[test]
fn test_square_is_one_face() {
    let faces = planar_arrangement(&lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]), O, N);
    assert_eq!(faces.len(), 1);
    let (outer, holes) = &faces[0];
    assert_eq!(outer.len(), 4);
    assert!(holes.is_empty());
    assert!((area(outer) - 16.0).abs() < 1e-6);
}

#[test]
fn test_diagonal_splits_square_into_two_triangles() {
    let mut segs = lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]);
    segs.push((V(0., 0.), V(4., 4.)));
    let faces = planar_arrangement(&segs, O, N);
    assert_eq!(faces.len(), 2);
    for (outer, holes) in &faces {
        assert_eq!(outer.len(), 3);
        assert!(holes.is_empty());
        assert!((area(outer) - 8.0).abs() < 1e-6);
    }
}

fn crossing_rectangles() -> Vec<(DVec3, DVec3)> {
    let mut segs = lp(&[(0., 0.), (4., 0.), (4., 3.), (0., 3.)]);
    segs.extend(lp(&[(2., 1.), (6., 1.), (6., 5.), (2., 5.)]));
    segs
}

#[test]
fn test_crossing_rectangles_make_three_faces() {
    let faces = planar_arrangement(&crossing_rectangles(), O, N);
    assert_eq!(faces.len(), 3);
    let mut areas: Vec<f64> = faces
        .iter()
        .map(|(o, _)| (area(o) * 1000.0).round() / 1000.0)
        .collect();
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(areas[0], 4.0);
}

#[test]
fn test_inner_square_nests_as_a_hole_of_the_ring() {
    let mut segs = lp(&[(0., 0.), (6., 0.), (6., 6.), (0., 6.)]);
    segs.extend(lp(&[(2., 2.), (4., 2.), (4., 4.), (2., 4.)]));
    let faces = planar_arrangement(&segs, O, N);
    assert_eq!(faces.len(), 2);
    let ring = faces
        .iter()
        .max_by(|a, b| area(&a.0).partial_cmp(&area(&b.0)).unwrap())
        .unwrap();
    let inner = faces
        .iter()
        .min_by(|a, b| area(&a.0).partial_cmp(&area(&b.0)).unwrap())
        .unwrap();
    assert_eq!(ring.1.len(), 1);
    assert!((area(&ring.1[0]) - 4.0).abs() < 1e-6);
    assert!(inner.1.is_empty());
    assert!((area(&inner.0) - 4.0).abs() < 1e-6);
}

#[test]
fn test_spur_is_pruned_from_the_face() {
    let mut segs = lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]);
    segs.push((V(2., 0.), V(2., 2.)));
    let faces = planar_arrangement(&segs, O, N);
    assert_eq!(faces.len(), 1);
    assert!((area(&faces[0].0) - 16.0).abs() < 1e-6);
}

#[test]
fn test_doubled_edge_collapses() {
    let mut segs = lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]);
    segs.push((V(0., 0.), V(4., 0.)));
    let faces = planar_arrangement(&segs, O, N);
    assert_eq!(faces.len(), 1);
    assert!((area(&faces[0].0) - 16.0).abs() < 1e-6);
}

#[test]
fn test_open_loop_makes_no_face() {
    let segs = [
        (V(0., 0.), V(4., 0.)),
        (V(4., 0.), V(4., 4.)),
        (V(4., 4.), V(0., 4.)),
    ];
    assert!(planar_arrangement(&segs, O, N).is_empty());
}

fn flat_scene(
    segs: &[(DVec3, DVec3)],
) -> (xtrude_model::scene::Scene, xtrude_model::history::History) {
    let (mut s, mut h) = scene_history();
    for &(a, b) in segs {
        h.execute(&mut s, &mut AddEdgeCommand::new(a, b));
    }
    (s, h)
}

#[test]
fn test_command_rebuilds_crossing_rectangles() {
    let segs = crossing_rectangles();
    let (mut s, mut h) = flat_scene(&segs);
    assert_eq!(nfaces(&s), 0);
    let mut cmd = RebuildPlanarFacesCommand::new();
    h.execute(&mut s, &mut cmd);
    assert!(cmd.flat && cmd.rebuilt == 3);
    assert_eq!(nfaces(&s), 3);
    assert!(nedges(&s) > segs.len());
}

#[test]
fn test_command_undo_restores_raw_edges() {
    let (mut s, mut h) = flat_scene(&lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]));
    let before = nedges(&s);
    h.execute(&mut s, &mut RebuildPlanarFacesCommand::new());
    assert_eq!(nfaces(&s), 1);
    h.undo(&mut s);
    assert_eq!(nfaces(&s), 0);
    assert_eq!(nedges(&s), before);
}

#[test]
fn test_command_skips_non_flat_mesh() {
    let mut segs = lp(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]);
    segs.push((V3(0., 0., 0.), V3(0., 0., 3.)));
    let (mut s, mut h) = flat_scene(&segs);
    let mut cmd = RebuildPlanarFacesCommand::new();
    h.execute(&mut s, &mut cmd);
    assert!(!cmd.flat);
    assert_eq!(nfaces(&s), 0);
}
