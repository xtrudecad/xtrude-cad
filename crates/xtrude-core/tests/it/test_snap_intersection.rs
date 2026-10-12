// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The green X where two edges / guide lines actually cross.

use crate::snap_common;
use snap_common::*;

fn snap(edges: &[SnapEdge], cand: DVec3) -> SnapResult {
    compute_snap(&query(cand), edges)
}

fn two_guides() -> Vec<SnapEdge> {
    vec![
        guide(v(0., 1., 0.), v(1., 0., 0.)),
        guide(v(2., 0., 0.), v(0., 1., 0.)),
    ]
}

#[test]
fn test_two_crossing_guides_snap_to_the_x() {
    let r = snap(&two_guides(), v(2.0, 1.0, 0.0));
    assert_eq!(r.kind, SnapKind::Intersection);
    assert!((r.point - v(2., 1., 0.)).length() < 1e-3);
}

#[test]
fn test_three_long_guides_find_all_triangle_vertices() {
    let es = [
        guide(v(0., 0., 0.), v(1., 0., 0.)),
        guide(v(0., 0., 0.), v(0., 1., 0.)),
        guide(v(10., 0., 0.), v(1., -1., 0.)),
    ];
    for p in [v(0., 0., 0.), v(10., 0., 0.), v(0., 10., 0.)] {
        let r = snap(&es, p);
        assert!(
            matches!(r.kind, SnapKind::Intersection | SnapKind::Origin),
            "{p:?}"
        );
    }
}

#[test]
fn test_crossing_beats_on_edge_just_beside_it() {
    let r = snap(&two_guides(), v(2.06, 1.05, 0.0));
    assert_eq!(r.kind, SnapKind::Intersection);
    assert!((r.point - v(2., 1., 0.)).length() < 1e-3);
}

#[test]
fn test_crossing_beats_first_point_inference() {
    let mut q = query(v(2.0, 1.0, 0.0));
    q.acquired_point = Some(v(0., 1., 0.));
    let r = compute_snap(&q, &two_guides());
    assert_eq!(r.kind, SnapKind::Intersection);
    assert!((r.point - v(2., 1., 0.)).length() < 1e-3);
}

#[test]
fn test_guide_crossing_a_model_edge_snaps() {
    let es = [
        edge(v(0., 0., 0.), v(3., 3., 0.)),
        guide(v(0., 1., 0.), v(1., 0., 0.)),
    ];
    let r = snap(&es, v(1.0, 1.0, 0.0));
    assert_eq!(r.kind, SnapKind::Intersection);
    assert!((r.point - v(1., 1., 0.)).length() < 1e-3);
}

#[test]
fn test_endpoint_wins_over_intersection() {
    let es = [
        edge(v(0., 0., 0.), v(2., 1., 0.)),
        edge(v(2., 1., 0.), v(3., 0., 0.)),
        guide(v(0., 1., 0.), v(1., 0., 0.)),
    ];
    let r = snap(&es, v(2.01, 1.0, 0.0));
    assert_eq!(r.kind, SnapKind::Endpoint);
    assert!((r.point - v(2., 1., 0.)).length() < 1e-4);
}

#[test]
fn test_single_guide_still_snaps_on_edge() {
    let r = snap(&[guide(v(0., 1., 0.), v(1., 0., 0.))], v(3.5, 1.0, 0.0));
    assert_eq!(r.kind, SnapKind::OnEdge);
}

#[test]
fn test_far_from_the_crossing_is_still_on_edge() {
    let r = snap(&two_guides(), v(3.5, 1.0, 0.0));
    assert_eq!(r.kind, SnapKind::OnEdge);
}

#[test]
fn test_parallel_guides_are_not_an_intersection() {
    let es = [
        guide(v(0., 1., 0.), v(1., 0., 0.)),
        guide(v(0., 2., 0.), v(1., 0., 0.)),
    ];
    assert_ne!(snap(&es, v(3.5, 1.0, 0.0)).kind, SnapKind::Intersection);
}

#[test]
fn test_skew_guides_do_not_invent_a_point() {
    let es = [
        guide(v(0., 1., 0.), v(1., 0., 0.)),
        guide(v(2., 0., 1.), v(0., 1., 0.)),
    ];
    assert_ne!(snap(&es, v(2.0, 1.0, 0.0)).kind, SnapKind::Intersection);
}

#[test]
fn test_occluded_crossing_is_not_offered() {
    let occ = |p: DVec3| (p - v(2., 1., 0.)).length() < 0.5;
    let mut q = query(v(2.05, 1.05, 0.0));
    q.is_occluded = Some(&occ);
    assert_ne!(compute_snap(&q, &two_guides()).kind, SnapKind::Intersection);
}
