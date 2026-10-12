// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The world origin is a reference like any corner under an axis lock.

use crate::snap_common;
use snap_common::*;
use xtrude_core::Axis;

fn square() -> Vec<SnapEdge> {
    vec![
        edge(v(2., 2., 0.), v(6., 2., 0.)),
        edge(v(6., 2., 0.), v(6., 6., 0.)),
        edge(v(6., 6., 0.), v(2., 6., 0.)),
        edge(v(2., 6., 0.), v(2., 2., 0.)),
    ]
}

const START: DVec3 = DVec3::new(1.0, 3.0, 0.0);

fn snap(cand: DVec3, start: DVec3, lock: Option<Axis>) -> SnapResult {
    let project = proj_factory(cand);
    let mut q = query(cand);
    q.start_point = Some(start);
    q.axis_lock = lock;
    if lock.is_some() {
        q.project_onto_line = Some(&project);
    }
    compute_snap(&q, &square())
}

fn origin_or_from_point(k: &SnapKind) -> bool {
    matches!(k, SnapKind::Origin | SnapKind::FromPoint)
}

#[test]
fn test_the_origin_is_found_under_an_axis_lock() {
    let r = snap(v(0.04, 0.03, 0.), START, Some(Axis::X));
    assert!(origin_or_from_point(&r.kind), "{:?}", r.kind);
    assert!(r.point.x.abs() < 1e-6 && (r.point.y - 3.0).abs() < 1e-6);
}

#[test]
fn test_a_corner_still_is() {
    let r = snap(v(2.04, 2.03, 0.), START, Some(Axis::X));
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point.x - 2.0).abs() < 1e-6);
}

#[test]
fn test_a_midpoint_still_is() {
    let r = snap(v(4.04, 2.03, 0.), START, Some(Axis::X));
    assert_eq!(r.kind, SnapKind::Midpoint);
    assert!((r.point.x - 4.0).abs() < 1e-6);
}

#[test]
fn test_without_a_lock_the_origin_is_the_origin() {
    assert_eq!(snap(v(0.04, 0.03, 0.), START, None).kind, SnapKind::Origin);
}

#[test]
fn test_the_origin_does_not_hijack_a_far_cursor() {
    assert_eq!(
        snap(v(9.0, 3.0, 0.), START, Some(Axis::X)).kind,
        SnapKind::Axis
    );
}

#[test]
fn test_the_side_you_draw_from_makes_no_difference() {
    for start in [v(1., 3., 0.), v(8., 3., 0.)] {
        let r = snap(v(2.04, 2.03, 0.), start, Some(Axis::X));
        assert_eq!(r.kind, SnapKind::FromPoint);
        assert!((r.point.x - 2.0).abs() < 1e-6 && (r.point.y - 3.0).abs() < 1e-6);
    }
}

#[test]
fn test_the_origin_too_from_either_side() {
    for start in [v(-5., 3., 0.), v(8., 3., 0.)] {
        let r = snap(v(0.04, 0.03, 0.), start, Some(Axis::X));
        assert!(origin_or_from_point(&r.kind), "{start:?} {:?}", r.kind);
        assert!(r.point.x.abs() < 1e-6);
    }
}

#[test]
fn test_it_works_on_the_other_axes_too() {
    let cand = v(0.03, 0.04, 0.);
    let r = snap(cand, v(3., 1., 0.), Some(Axis::Y));
    assert!(origin_or_from_point(&r.kind), "{:?}", r.kind);
    assert!(r.point.y.abs() < 1e-6);
}
