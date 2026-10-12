// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shift's lock offers the same snaps the arrow lock does.

use crate::snap_common;
use snap_common::*;

fn box_edges() -> Vec<SnapEdge> {
    vec![
        edge(v(2., -1., 0.), v(6., -1., 0.)),
        edge(v(6., -1., 0.), v(6., 3., 0.)),
    ]
}

fn snap(cand: DVec3, edges: &[SnapEdge]) -> SnapResult {
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = Some(v(0., 0., 0.));
    q.project_onto_line = Some(&project);
    q.shift_lock_dir = Some(v(1., 0., 0.));
    compute_snap(&q, edges)
}

#[test]
fn test_a_midpoint_is_reachable_while_shift_holds_the_axis() {
    assert_eq!(
        snap(v(6.0, 1.0, 0.0), &box_edges()).kind,
        SnapKind::Midpoint
    );
}

#[test]
fn test_so_is_the_crossing_with_another_edge() {
    assert_eq!(
        snap(v(6.0, 0.0, 0.0), &box_edges()).kind,
        SnapKind::Intersection
    );
}

#[test]
fn test_and_a_corner_lines_up_onto_the_locked_line() {
    assert_eq!(
        snap(v(6.0, -1.0, 0.0), &box_edges()).kind,
        SnapKind::FromPoint
    );
}

#[test]
fn test_a_vertex_on_the_line_still_wins_outright() {
    let r = snap(v(4.0, 0.0, 0.0), &[edge(v(4., 0., 0.), v(4., 2., 0.))]);
    assert_eq!(r.kind, SnapKind::Endpoint);
    assert!((r.point.x - 4.0).abs() < 1e-6);
}

#[test]
fn test_the_lock_still_holds_where_there_is_nothing_to_snap_to() {
    let r = snap(v(9.0, 0.6, 0.0), &[]);
    assert_eq!(r.kind, SnapKind::Reference);
    assert!(r.point.y.abs() < 1e-6);
}

#[test]
fn test_it_reads_the_lock_line_both_ways() {
    let left = [
        edge(v(-6., -1., 0.), v(-2., -1., 0.)),
        edge(v(-6., -1., 0.), v(-6., 3., 0.)),
    ];
    assert_eq!(snap(v(-6.0, 1.0, 0.0), &left).kind, SnapKind::Midpoint);
}
