// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Perpendicular is square to the edge IN THE PLANE being drawn on (a slope,
//! a vertical face).

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::{direction_from_edge, RefMode};

#[test]
fn test_on_the_ground_it_is_exactly_as_before() {
    for (a, b) in [
        ((0., 0., 0.), (3., 0., 0.)),
        ((1., 2., 0.), (4., 7., 0.)),
        ((0., 0., 5.), (2., -3., 5.)),
        ((0., 0., 0.), (1., 1., 1.)),
    ] {
        let (pa, pb) = (v(a.0, a.1, a.2), v(b.0, b.1, b.2));
        let before = v(-(b.1 - a.1), b.0 - a.0, 0.0).normalize();
        let now = direction_from_edge(pa, pb, RefMode::Perpendicular, Some(v(0., 0., 1.))).unwrap();
        assert!((now - before).length() < 1e-6, "{a:?}->{b:?}");
        let no_plane = direction_from_edge(pa, pb, RefMode::Perpendicular, None).unwrap();
        assert!((no_plane - before).length() < 1e-6);
    }
}

#[test]
fn test_on_a_slope_the_perpendicular_lies_on_the_slope() {
    let normal = v(0.0, -1.0, 2.0).normalize();
    let perp = direction_from_edge(
        v(0., 0., 0.),
        v(4., 0., 0.),
        RefMode::Perpendicular,
        Some(normal),
    )
    .unwrap();
    assert!(perp.dot(normal).abs() < 1e-6);
    assert!(perp.dot(v(1., 0., 0.)).abs() < 1e-6);
    assert!(perp.z.abs() > 0.4, "came out horizontal again: {perp:?}");
}

#[test]
fn test_on_a_vertical_face_the_perpendicular_goes_up() {
    let perp = direction_from_edge(
        v(0., 0., 0.),
        v(3., 0., 0.),
        RefMode::Perpendicular,
        Some(v(0., 1., 0.)),
    )
    .unwrap();
    assert!((perp.z.abs() - 1.0).abs() < 1e-6);
}

#[test]
fn test_an_edge_piercing_the_plane_has_no_perpendicular() {
    assert!(direction_from_edge(
        v(0., 0., 0.),
        v(0., 0., 2.),
        RefMode::Perpendicular,
        Some(v(0., 0., 1.))
    )
    .is_none());
}

#[test]
fn test_parallel_does_not_depend_on_the_plane() {
    let expected = v(1., 1., 1.).normalize();
    for normal in [None, Some(v(0., 0., 1.)), Some(v(0., 1., 0.))] {
        let d =
            direction_from_edge(v(0., 0., 0.), v(1., 1., 1.), RefMode::Parallel, normal).unwrap();
        assert!((d - expected).length() < 1e-6);
    }
}

#[test]
fn test_the_slope_angle_is_kept() {
    let normal = v(0.0, -1.0, 2.0).normalize();
    let perp = direction_from_edge(
        v(0., 0., 0.),
        v(4., 0., 0.),
        RefMode::Perpendicular,
        Some(normal),
    )
    .unwrap();
    let degrees = perp.z.abs().asin().to_degrees();
    assert!((degrees - 1f64.atan2(2.0).to_degrees()).abs() < 1e-4);
}

const RISE: f64 = 0.35;

fn slope() -> (Vec<SnapEdge>, DVec3) {
    let e = [
        v(0., 0., 0.),
        v(4., 0., 0.),
        v(4., 4., RISE),
        v(0., 4., RISE),
    ];
    let edges = (0..4).map(|i| edge(e[i], e[(i + 1) % 4])).collect();
    (edges, v(4., 0., 0.).cross(v(0., 4., RISE)).normalize())
}

fn snap_on_slope(
    edges: &[SnapEdge],
    normal: Option<DVec3>,
    start: DVec3,
    cursor: DVec3,
) -> SnapResult {
    let project = proj_factory(cursor);
    let mut q = query(cursor);
    q.start_point = Some(start);
    q.project_onto_line = Some(&project);
    q.work_plane_normal = normal;
    compute_snap(&q, edges)
}

#[test]
fn test_crossing_the_slope_reaches_the_opposite_edge() {
    let (edges, normal) = slope();
    let cursor = v(2., 3.9, RISE * 3.9 / 4.0);
    let r = snap_on_slope(&edges, Some(normal), v(2., 0., 0.), cursor);
    assert!(
        (r.point.z - RISE).abs() < 1e-6,
        "the point was left off the slope: {:?}",
        r.point
    );
    assert!((r.point.y - 4.0).abs() < 1e-6);
    assert_eq!(r.kind, SnapKind::Intersection);
}

#[test]
fn test_without_the_plane_the_magenta_stayed_at_ground_level() {
    let (edges, _) = slope();
    let cursor = v(2., 3.9, RISE * 3.9 / 4.0);
    let r = snap_on_slope(&edges, None, v(2., 0., 0.), cursor);
    assert_eq!(r.kind, SnapKind::Reference);
    assert!(r.point.z.abs() < 1e-9);
}
