// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The acquired point's alignment holds mid-operation, below the named points.

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::LinearMode;

const START: DVec3 = DVec3::new(0.0, 0.0, 0.0);
const CORNER: DVec3 = DVec3::new(6.43, 0.0, 0.0);

fn snap(
    cand: DVec3,
    start: Option<DVec3>,
    acquired: Option<DVec3>,
    edges: Option<Vec<SnapEdge>>,
    mode: LinearMode,
) -> SnapResult {
    let project = proj_factory(cand);
    let es = edges.unwrap_or_else(|| vec![edge(START, CORNER)]);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = start;
    if start.is_some() {
        q.project_onto_line = Some(&project);
    }
    q.acquired_point = acquired;
    q.linear_mode = mode;
    compute_snap(&q, &es)
}

fn snap_default(cand: DVec3) -> SnapResult {
    snap(cand, Some(START), Some(CORNER), None, LinearMode::All)
}

#[test]
fn test_the_length_stays_on_the_corner_while_you_pull_the_depth() {
    for depth in [0.40, 0.90, 1.50] {
        let r = snap_default(v(6.37, depth, 0.0));
        assert_eq!(r.kind, SnapKind::FromPoint, "{depth}");
        assert!((r.point.x - 6.43).abs() < 1e-6, "the wall's end");
    }
}

#[test]
fn test_without_an_acquired_point_nothing_changes() {
    let r = snap(v(6.37, 0.90, 0.0), Some(START), None, None, LinearMode::All);
    assert_ne!(r.kind, SnapKind::FromPoint);
    assert!((r.point.x - 6.37).abs() < 1e-6);
}

#[test]
fn test_a_named_point_still_wins() {
    let es = vec![edge(v(6.43, 0.80, 0.0), v(6.43, 1.00, 0.0))];
    let r = snap(
        v(6.43, 0.90, 0.0),
        Some(START),
        Some(CORNER),
        Some(es),
        LinearMode::All,
    );
    assert_eq!(r.kind, SnapKind::Midpoint);
}

#[test]
fn test_alt_switches_it_off_like_every_other_linear_inference() {
    let r = snap(
        v(6.37, 0.90, 0.0),
        Some(START),
        Some(CORNER),
        None,
        LinearMode::Off,
    );
    assert_ne!(r.kind, SnapKind::FromPoint);
}

#[test]
fn test_it_still_works_before_the_first_click() {
    let r = snap(
        v(6.37, 0.40, 0.0),
        None,
        Some(CORNER),
        None,
        LinearMode::All,
    );
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point.x - 6.43).abs() < 1e-6);
}

#[test]
fn test_a_midpoint_beats_the_alignment_line_on_the_first_click() {
    let (a, b) = (v(2.0, 1.0, 0.0), v(6.0, 1.0, 0.0));
    let mid = v(4.0, 1.0, 0.0);
    let mut q = query_with(mid, &w2p_up);
    q.acquired_point = Some(a);
    let r = compute_snap(&q, &[edge(a, b)]);
    assert_eq!(r.kind, SnapKind::Midpoint);
    assert!((r.point.x - 4.0).abs() < 1e-6);
}

#[test]
fn test_the_alignment_still_answers_where_no_named_point_does() {
    let cand = v(5.0, 1.02, 0.0);
    let mut q = query_with(cand, &w2p_up);
    q.acquired_point = Some(v(2.0, 1.0, 0.0));
    let r = compute_snap(&q, &[]);
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point.y - 1.0).abs() < 1e-6);
}

#[test]
fn test_pausing_on_a_point_does_not_make_it_unsnappable() {
    let (a, b) = (v(23.0, 6.0, 0.0), v(36.0, 6.0, 0.0));
    let mid = (a + b) * 0.5;
    let project = move |s: DVec3, d: DVec3| s + d * (mid - s).dot(d);
    let mut q = query_with(mid, &w2p_up);
    q.start_point = Some(v(22.0, -0.1, 0.0));
    q.project_onto_line = Some(&project);
    q.acquired_point = Some(mid);
    let r = compute_snap(&q, &[edge(a, b)]);
    assert_eq!(r.kind, SnapKind::Midpoint);
    assert!((r.point - mid).length() < 1e-6);
}

#[test]
fn test_but_it_still_carries_you_past_the_point() {
    let mid = v(29.5, 6.0, 0.0);
    let start = v(22.0, -0.1, 0.0);
    let far = start + (mid - start) * 1.6;
    let project = move |s: DVec3, d: DVec3| s + d * (far - s).dot(d);
    let mut q = query_with(far, &w2p_up);
    q.start_point = Some(start);
    q.project_onto_line = Some(&project);
    q.acquired_point = Some(mid);
    assert_eq!(compute_snap(&q, &[]).kind, SnapKind::ThroughPoint);
}
