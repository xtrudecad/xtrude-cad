// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Snap to the collinear continuation of a nearby edge.

use crate::snap_common;
use snap_common::*;

fn snap(edges: &[SnapEdge], cand: DVec3, start: DVec3) -> SnapResult {
    let mut q = query(cand);
    q.start_point = Some(start);
    compute_snap(&q, edges)
}

#[test]
fn test_extension_snaps_onto_the_edge_line() {
    let es = [edge(v(0., 0., 0.), v(2., 0., 0.))];
    let r = snap(&es, v(3.0, 0.05, 0.0), v(2., 0., 0.));
    assert_eq!(r.kind, SnapKind::Extension);
    assert!(r.point.y.abs() < 1e-9);
    assert!((r.point.x - 3.0).abs() < 1e-9);
    assert!(r.guide.is_some());
}

#[test]
fn test_extension_does_not_fire_off_the_line() {
    let es = [edge(v(0., 0., 0.), v(2., 0., 0.))];
    let r = snap(&es, v(3.0, 0.5, 0.0), v(2., 0., 0.));
    assert_ne!(r.kind, SnapKind::Extension);
}

#[test]
fn test_extension_not_on_the_segment_itself() {
    let es = [edge(v(0., 0., 0.), v(2., 0., 0.))];
    let r = snap(&es, v(1.0, 0.02, 0.0), v(0., 0., 0.));
    assert_ne!(r.kind, SnapKind::Extension);
}

#[test]
fn test_endpoint_wins_over_extension() {
    let es = [edge(v(0., 0., 0.), v(2., 0., 0.))];
    let r = snap(&es, v(2.01, 0.01, 0.0), v(0., 0., 0.));
    assert_eq!(r.kind, SnapKind::Endpoint);
}

#[test]
fn test_extension_snaps_to_crossing_with_perpendicular_edge() {
    let es = [
        edge(v(0., 0., 0.), v(10., 0., 0.)),
        edge(v(10.2, -1., 0.), v(10.2, 1., 0.)),
    ];
    let r = snap(&es, v(10.15, 0.02, 0.0), v(10., 0., 0.));
    assert_eq!(r.kind, SnapKind::Intersection);
    assert!((r.point.x - 10.2).abs() < 1e-4);
    assert!(r.point.y.abs() < 1e-4);
    assert!(r.guide.is_some());
}

#[test]
fn test_extension_needs_a_collinear_draw_direction() {
    let es = [edge(v(0., 0., 0.), v(10., 0., 0.))];
    let r = snap(&es, v(10.1, 0.5, 0.0), v(10., 0., 0.));
    assert!(!matches!(
        r.kind,
        SnapKind::Extension | SnapKind::Intersection
    ));
}

fn perp_snap(es: &[SnapEdge], cand: DVec3, start: DVec3) -> SnapResult {
    let proj = proj_factory(cand);
    let mut q = query(cand);
    q.start_point = Some(start);
    q.project_onto_line = Some(&proj);
    q.inference_angle_deg = 3.0;
    compute_snap(&q, es)
}

#[test]
fn test_perpendicular_to_angled_edge_locks() {
    let es = [
        edge(v(0., 0., 0.), v(3., 3., 0.)),
        edge(v(1., -1., 0.), v(4., 2., 0.)),
    ];
    let ang = (135.0f64 + 1.5).to_radians();
    let cand = v(ang.cos() * 2.0, ang.sin() * 2.0, 0.0);
    let r = perp_snap(&es, cand, v(0., 0., 0.));
    assert_eq!(r.kind, SnapKind::Reference);
    let dirn = r.point.normalize();
    assert!(dirn.dot(v(-1., 1., 0.).normalize()).abs() > 0.999);
}

#[test]
fn test_no_perpendicular_lock_when_not_square() {
    let es = [edge(v(0., 0., 0.), v(3., 3., 0.))];
    let r = perp_snap(&es, v(2.0, 0.3, 0.0), v(0., 0., 0.));
    assert_ne!(r.kind, SnapKind::Reference);
}

#[test]
fn test_perpendicular_to_axis_aligned_wall_locks() {
    let es = [edge(v(0., 0., 0.), v(5., 0., 0.))];
    let r = perp_snap(&es, v(2.0, 2.0, 0.0), v(2., 0., 0.));
    assert_eq!(r.kind, SnapKind::Reference);
    assert!((r.point - v(2., 0., 0.)).normalize().y.abs() > 0.999);
}
