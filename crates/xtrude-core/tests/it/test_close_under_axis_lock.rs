// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An axis lock must not trap you inside the polyline.

use crate::snap_common;
use snap_common::*;
use xtrude_core::Axis;

const FIRST: DVec3 = DVec3::new(2.0, 5.0, 0.0);
const ACTUAL: DVec3 = DVec3::new(6.0, 5.0, 0.0);

fn snap(cursor: DVec3, lock: Option<Axis>, first: DVec3, edges: &[SnapEdge]) -> SnapResult {
    let project = proj_factory(cursor);
    let mut q = query_with(cursor, &w2p_up);
    q.start_point = Some(ACTUAL);
    q.chain_first_point = Some(first);
    q.axis_lock = lock;
    q.project_onto_line = Some(&project);
    compute_snap(&q, edges)
}

#[test]
fn test_the_chain_closes_with_the_axis_locked() {
    let r = snap(v(2.02, 5.01, 0.0), Some(Axis::X), FIRST, &[]);
    assert_eq!(r.kind, SnapKind::Close);
    assert!((r.point.x - 2.0).abs() < 1e-6 && (r.point.y - 5.0).abs() < 1e-6);
}

#[test]
fn test_and_still_closes_without_one() {
    assert_eq!(
        snap(v(2.02, 5.01, 0.0), None, FIRST, &[]).kind,
        SnapKind::Close
    );
}

#[test]
fn test_a_first_point_off_the_lock_line_does_not_break_the_lock() {
    let r = snap(v(2.02, 6.01, 0.0), Some(Axis::X), v(2.0, 6.0, 0.0), &[]);
    assert_ne!(r.kind, SnapKind::Close);
}

#[test]
fn test_a_cursor_nowhere_near_it_still_gets_the_axis() {
    assert_eq!(
        snap(v(4.5, 5.0, 0.0), Some(Axis::X), FIRST, &[]).kind,
        SnapKind::Axis
    );
}

#[test]
fn test_an_endpoint_on_the_line_is_unaffected() {
    let es = [edge(v(3.0, 5.0, 0.0), v(3.0, 7.0, 0.0))];
    assert_eq!(
        snap(v(3.01, 5.01, 0.0), Some(Axis::X), FIRST, &es).kind,
        SnapKind::Endpoint
    );
}
