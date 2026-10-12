// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — the first corner lines up with the hovered corner along an
//! axis. (The viewport's hover bookkeeping and pseudo-edges live with the
//! app.)

use crate::snap_common;
use snap_common::*;
use xtrude_core::Axis;

/// An elevation: x → right, z → up.
fn elev(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.z * 100.0))
}

fn panel_with_notch() -> Vec<SnapEdge> {
    vec![
        edge(v(1., 0., 0.), v(1., 0., 2.)),
        edge(v(2., 0., 0.), v(2., 0., 2.)),
        edge(v(1., 0., 2.), v(2., 0., 2.)),
    ]
}

fn snap(cand: DVec3, acquired: Option<DVec3>, acquired_points: Vec<DVec3>) -> SnapResult {
    let mut q = query_with(cand, &elev);
    q.acquired_point = acquired;
    q.acquired_points = acquired_points;
    compute_snap(&q, &panel_with_notch())
}

#[test]
fn test_the_first_corner_lines_up_with_the_hovered_corner_along_an_axis() {
    let r = snap(v(3.5, 0., 2.03), Some(v(2., 0., 2.)), vec![]);
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point.z - 2.0).abs() < 1e-9 && (r.point.x - 3.5).abs() < 1e-9);
    assert!((r.guide.unwrap().0 - v(2., 0., 2.)).length() < 1e-9);
    assert_eq!(r.guide_color, Some(Axis::X.color()));
}

#[test]
fn test_it_stays_quiet_off_the_axis_lines_and_without_a_corner() {
    assert_ne!(
        snap(v(3.5, 0., 2.5), Some(v(2., 0., 2.)), vec![]).kind,
        SnapKind::FromPoint
    );
    assert_ne!(
        snap(v(3.5, 0., 2.03), None, vec![]).kind,
        SnapKind::FromPoint
    );
}

#[test]
fn test_the_corner_itself_is_still_an_endpoint() {
    assert_eq!(
        snap(v(2.01, 0., 2.01), Some(v(2., 0., 2.)), vec![]).kind,
        SnapKind::Endpoint
    );
}

#[test]
fn test_two_encouraged_points_pin_the_cursor_where_their_axis_lines_cross() {
    let (lintel_end, jamb_foot) = (v(2., 0., 2.), v(5., 0., 0.));
    let r = snap(
        v(4.97, 0., 2.04),
        Some(jamb_foot),
        vec![lintel_end, jamb_foot],
    );
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point.x - 5.0).abs() < 1e-9 && (r.point.z - 2.0).abs() < 1e-9);
    let gs = r.guides.clone().unwrap();
    assert_eq!(gs.len(), 1);
    let mut starts = [r.guide.unwrap().0, gs[0].0];
    starts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
    assert!((starts[0] - lintel_end).length() < 1e-6 && (starts[1] - jamb_foot).length() < 1e-6);
    let colours = [r.guide_color.unwrap(), gs[0].2];
    assert!(colours.contains(&Axis::X.color()) && colours.contains(&Axis::Z.color()));
}
