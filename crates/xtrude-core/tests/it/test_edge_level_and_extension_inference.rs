// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! On the edge level with the acquired corner; the extension with a wall
//! behind the cursor.

use crate::snap_common;
use snap_common::*;

#[test]
fn test_the_edge_offers_the_point_level_with_the_acquired_corner() {
    let wall = edge(v(0., 0., 0.), v(0., 5., 0.));
    let cursor = v(0.03, 2.04, 0.);
    let project = proj_factory(cursor);
    let mut q = query_with(cursor, &w2p_up);
    q.start_point = Some(v(3., 5., 0.));
    q.project_onto_line = Some(&project);
    q.acquired_point = Some(v(3., 2., 0.));
    let r = compute_snap(&q, &[wall]);
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!(r.point.x.abs() < 1e-6 && (r.point.y - 2.0).abs() < 1e-6);
    assert!(r.guide.is_some());
}

#[test]
fn test_without_an_acquired_point_it_is_still_on_edge() {
    let wall = edge(v(0., 0., 0.), v(0., 5., 0.));
    let cursor = v(0.03, 2.04, 0.);
    let project = proj_factory(cursor);
    let mut q = query_with(cursor, &w2p_up);
    q.start_point = Some(v(3., 5., 0.));
    q.project_onto_line = Some(&project);
    assert_eq!(compute_snap(&q, &[wall]).kind, SnapKind::OnEdge);
}

/// Front view: x right, z up; y is depth.
fn front(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.z * 100.0))
}

/// The cursor ray of the front view: the point of the line at the pixel.
fn ray_projector(px: (f64, f64)) -> impl Fn(DVec3, DVec3) -> DVec3 {
    let (cx, cz) = (px.0 / 100.0, -px.1 / 100.0);
    move |s: DVec3, d: DVec3| {
        let den = d.x * d.x + d.z * d.z;
        if den < 1e-12 {
            return s;
        }
        let t = ((cx - s.x) * d.x + (cz - s.z) * d.z) / den;
        s + d * t
    }
}

#[test]
fn test_the_extension_shows_with_a_wall_behind_the_cursor() {
    let rafter = edge(v(0., 0., 0.), v(2., 0., 1.));
    let on_screen = v(3., 0., 1.5);
    let behind = v(3., 5., 1.5);
    let px = front(on_screen).unwrap();
    let project = ray_projector(px);
    let mut q = SnapQuery::new(behind, px, &front, 9.0);
    q.edge_threshold_px = Some(14.0);
    q.start_point = Some(v(2., 0., 1.));
    q.project_onto_line = Some(&project);
    let r = compute_snap(&q, &[rafter]);
    assert_eq!(r.kind, SnapKind::Extension);
    assert!((r.point - v(3., 0., 1.5)).length() < 1e-6, "{:?}", r.point);
}
