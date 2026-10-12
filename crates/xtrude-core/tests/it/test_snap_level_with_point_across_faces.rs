// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — level with / in line with an encouraged point ACROSS the
//! face under the cursor. (The viewport tests live with the app.)

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::COLOR_ENDPOINT;
use xtrude_core::Axis;

fn plan(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.y * 100.0))
}

fn side(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.z * 100.0))
}

fn iso(p: DVec3) -> Option<(f64, f64)> {
    Some(((p.x - p.y) * 86.6, -(p.z + (p.x + p.y) * 0.5) * 100.0))
}

const REF: DVec3 = DVec3::new(3.5, 0.0, 2.2);

fn snap(
    cand: DVec3,
    w2p: fn(DVec3) -> Option<(f64, f64)>,
    reference: DVec3,
    normal: DVec3,
) -> SnapResult {
    let mut q = SnapQuery::new(cand, w2p(cand).unwrap(), &w2p, 9.0);
    q.edge_threshold_px = Some(14.0);
    q.face_under_cursor = true;
    q.acquired_point = Some(reference);
    q.work_plane_normal = Some(normal);
    compute_snap(&q, &[])
}

#[test]
fn test_level_with_the_corner_across_the_perpendicular_face() {
    let r = snap(v(8., 2.0, 2.23), iso, REF, v(1., 0., 0.));
    assert!(
        r.kind == SnapKind::Aligned && r.axis == Some(Axis::Z),
        "{r:?}"
    );
    assert!((r.point - v(8., 2.0, 2.2)).length() < 1e-6);
    assert_eq!(r.color, COLOR_ENDPOINT);
    let elbow = v(8., 0., 2.2);
    let g = r.guide.unwrap();
    assert!((g.0 - elbow).length() < 1e-6 && (g.1 - r.point).length() < 1e-6);
    assert_eq!(r.guide_color, Some(Axis::Y.color()));
    let gs = r.guides.unwrap();
    assert_eq!(gs.len(), 1);
    let (a, b, colour) = gs[0];
    assert!((a - REF).length() < 1e-6 && (b - elbow).length() < 1e-6);
    assert_eq!(colour, Axis::X.color());
}

#[test]
fn test_level_with_the_corner_across_the_opposite_face() {
    let r = snap(v(6.0, 5., 2.17), iso, REF, v(0., 1., 0.));
    assert!(r.kind == SnapKind::Aligned && r.axis == Some(Axis::Z));
    assert!((r.point - v(6.0, 5., 2.2)).length() < 1e-6);
    let gs = r.guides.unwrap();
    assert_eq!(gs.len(), 1);
    let (a, b, colour) = gs[0];
    assert!((a - REF).length() < 1e-6 && (b - v(3.5, 5., 2.2)).length() < 1e-6);
    assert_eq!(colour, Axis::Y.color());
    assert_eq!(r.guide_color, Some(Axis::X.color()));
}

#[test]
fn test_in_line_with_the_corner_on_the_floor_below_it() {
    let r = snap(v(3.53, 2.0, 0.), plan, REF, v(0., 0., 1.));
    assert!(r.kind == SnapKind::Aligned && r.axis == Some(Axis::X));
    assert!((r.point - v(3.5, 2.0, 0.)).length() < 1e-6);
}

#[test]
fn test_quiet_away_from_the_line_and_on_the_bare_ground() {
    assert_ne!(
        snap(v(8., 2.0, 2.5), iso, REF, v(1., 0., 0.)).kind,
        SnapKind::Aligned
    );
    let cand = v(3.53, 2.0, 0.);
    let mut q = SnapQuery::new(cand, plan(cand).unwrap(), &plan, 9.0);
    q.acquired_point = Some(REF);
    q.work_plane_normal = Some(v(0., 0., 1.));
    assert_ne!(compute_snap(&q, &[]).kind, SnapKind::Aligned);
}

#[test]
fn test_on_the_corners_own_face_the_axis_line_still_wins_unchanged() {
    let r = snap(v(6.0, 0., 2.23), iso, REF, v(0., -1., 0.));
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point - v(6.0, 0., 2.2)).length() < 1e-6);
    assert_eq!(r.guide_color, Some(Axis::X.color()));
}

#[test]
fn test_the_from_point_no_longer_leaves_the_face_the_cursor_is_on() {
    let cand = v(9.0, 5., 2.23);
    let mut q = SnapQuery::new(cand, side(cand).unwrap(), &side, 9.0);
    q.acquired_point = Some(REF);
    let before = compute_snap(&q, &[]);
    assert!(before.kind == SnapKind::FromPoint && before.point.y.abs() < 1e-9);
    let after = snap(cand, side, REF, v(0., 1., 0.));
    assert_eq!(after.kind, SnapKind::Aligned);
    assert!((after.point.y - 5.0).abs() < 1e-6 && (after.point.z - 2.2).abs() < 1e-6);
}

#[test]
fn test_a_line_piercing_the_face_offers_its_piercing_point() {
    let r = snap(v(3.52, 1.03, 0.), plan, v(3.5, 1.0, 2.2), v(0., 0., 1.));
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point - v(3.5, 1.0, 0.)).length() < 1e-6);
    assert_eq!(r.guide_color, Some(Axis::Z.color()));
}
