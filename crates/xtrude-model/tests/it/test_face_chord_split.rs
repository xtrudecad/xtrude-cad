// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Chord split + multi-cycle detection.

use crate::common;
use common::*;
use xtrude_core::geometry::{PolyFace, Segment};
use xtrude_core::topology::{find_cycles_through, split_face_by_chord};
use xtrude_core::DVec3;
use xtrude_model::edits::build_add_edge;

const A: DVec3 = DVec3::new(0., 0., 0.);
const B: DVec3 = DVec3::new(2., 0., 0.);
const C: DVec3 = DVec3::new(2., 2., 0.);
const D: DVec3 = DVec3::new(0., 2., 0.);

fn square() -> PolyFace {
    PolyFace::new(vec![A, B, C, D])
}

#[test]
fn test_chord_diagonal_splits_into_two_triangles() {
    let (la, lb) = split_face_by_chord(&square(), A, C).unwrap();
    assert_eq!((la.len(), lb.len()), (3, 3));
}

#[test]
fn test_chord_halves_not_inverted() {
    let mother = square();
    let n0 = mother.normal();
    let (la, lb) = split_face_by_chord(&mother, A, C).unwrap();
    for lp in [la, lb] {
        assert!(PolyFace::new(lp).normal().dot(n0) > 0.0);
    }
}

#[test]
fn test_adjacent_vertices_is_not_a_chord() {
    assert!(split_face_by_chord(&square(), A, B).is_none());
}

#[test]
fn test_chord_corner_to_edge_midpoint() {
    let (la, lb) = split_face_by_chord(&square(), A, V(1., 2.)).unwrap();
    let mut sizes = [la.len(), lb.len()];
    sizes.sort();
    assert_eq!(sizes, [3, 4]);
}

#[test]
fn test_chord_on_holed_face_is_skipped() {
    let f = PolyFace::with_holes(
        vec![A, B, C, D],
        vec![vec![V(0.5, 0.5), V(1.5, 0.5), V(1.5, 1.5), V(0.5, 1.5)]],
    );
    assert!(split_face_by_chord(&f, A, C).is_none());
}

#[test]
fn test_find_chord_split_picks_the_face() {
    let faces = [
        PolyFace::new(vec![V(10., 10.), V(11., 10.), V(11., 11.)]),
        square(),
    ];
    let found = faces
        .iter()
        .position(|f| split_face_by_chord(f, A, C).is_some());
    assert_eq!(found, Some(1));
}

fn square_edges() -> Vec<Segment> {
    vec![
        Segment::new(A, B),
        Segment::new(B, C),
        Segment::new(C, D),
        Segment::new(D, A),
    ]
}

#[test]
fn test_diagonal_bounds_two_cycles() {
    assert_eq!(find_cycles_through(&square_edges(), A, C, 2).len(), 2);
}

#[test]
fn test_closing_edge_is_single_cycle() {
    let edges = [Segment::new(A, B), Segment::new(B, C), Segment::new(C, D)];
    assert_eq!(find_cycles_through(&edges, D, A, 2).len(), 1);
}

#[test]
fn test_rectangle_then_diagonal_makes_two_triangles() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, &[A, B, C, D]);
    assert_eq!(vcount(&s), vec![4]);
    let c = build_add_edge(&s, A, C, true, None);
    exec(&mut s, &mut h, c);
    assert_eq!(vcount(&s), vec![3, 3]);
    assert_eq!(nedges(&s), 5);
}

#[test]
fn test_diagonal_split_undo_restores_rectangle() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, &[A, B, C, D]);
    let c = build_add_edge(&s, A, C, true, None);
    exec(&mut s, &mut h, c);
    assert!(h.undo(&mut s));
    assert_eq!(vcount(&s), vec![4]);
    assert_eq!(nedges(&s), 4);
}

#[test]
fn test_diagonal_split_redo() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, &[A, B, C, D]);
    let c = build_add_edge(&s, A, C, true, None);
    exec(&mut s, &mut h, c);
    h.undo(&mut s);
    assert!(h.redo(&mut s));
    assert_eq!(vcount(&s), vec![3, 3]);
}

#[test]
fn test_corner_to_edge_midpoint_end_to_end() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, &[A, B, C, D]);
    let c = build_add_edge(&s, A, V(1., 2.), true, None);
    exec(&mut s, &mut h, c);
    assert_eq!(vcount(&s), vec![3, 4]);
    assert_eq!(nedges(&s), 6);
}
