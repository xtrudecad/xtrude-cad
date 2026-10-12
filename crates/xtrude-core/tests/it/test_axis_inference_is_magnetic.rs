// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The axis inference magnetises; it does not merely light up.

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::LinearMode;
use xtrude_core::Axis;

const EPS: f64 = 1e-6;

/// Keeps the three axes apart on screen.
fn w2p3(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, p.y * 100.0 - p.z * 100.0))
}

fn snap(cand: DVec3, magnetic: f64, mode: LinearMode) -> SnapResult {
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p3);
    q.start_point = Some(v(0., 0., 0.));
    q.magnetic_axis_deg = Some(magnetic);
    q.project_onto_line = Some(&project);
    q.linear_mode = mode;
    compute_snap(&q, &[])
}

#[test]
fn test_a_cursor_near_the_red_axis_lands_on_it() {
    for off in [0.0, 0.02, 0.05, 0.08] {
        let r = snap(v(2.0, off, 0.0), 3.0, LinearMode::All);
        assert_eq!(r.kind, SnapKind::Axis, "{off}");
        assert_eq!(r.axis, Some(Axis::X));
        assert!(r.point.y.abs() < EPS);
    }
}

#[test]
fn test_drawing_deliberately_off_axis_is_still_possible() {
    let r = snap(v(2.0, 0.40, 0.0), 3.0, LinearMode::All);
    assert_ne!(r.kind, SnapKind::Axis);
    assert!((r.point.y - 0.40).abs() < EPS);
}

#[test]
fn test_the_green_and_blue_axes_too() {
    assert_eq!(
        snap(v(0.02, 2.0, 0.0), 3.0, LinearMode::All).axis,
        Some(Axis::Y)
    );
    assert_eq!(
        snap(v(0.02, 0.0, 2.0), 3.0, LinearMode::All).axis,
        Some(Axis::Z)
    );
}

#[test]
fn test_alt_switches_the_magnet_off() {
    let r = snap(v(2.0, 0.05, 0.0), 3.0, LinearMode::Off);
    assert_ne!(r.kind, SnapKind::Axis);
    assert!((r.point.y - 0.05).abs() < EPS);
}

#[test]
fn test_and_off_for_the_move_tool_as_well() {
    let r = snap(v(2.0, 0.15, 0.0), 15.0, LinearMode::Off);
    assert_ne!(r.kind, SnapKind::Axis, "Alt must reach Move's magnet too");
    assert!((r.point.y - 0.15).abs() < EPS);
}

#[test]
fn test_with_inferences_on_move_still_magnetises_widely() {
    let r = snap(v(2.0, 0.15, 0.0), 15.0, LinearMode::All);
    assert_eq!(r.kind, SnapKind::Axis);
    assert!(r.point.y.abs() < EPS);
}

#[test]
fn test_a_point_snap_still_beats_the_magnet() {
    let es = [edge(v(2.0, 0.03, 0.0), v(3.0, 0.03, 0.0))];
    let cand = v(2.0, 0.03, 0.0);
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p3);
    q.start_point = Some(v(0., 0., 0.));
    q.magnetic_axis_deg = Some(3.0);
    q.project_onto_line = Some(&project);
    let r = compute_snap(&q, &es);
    assert_eq!(r.kind, SnapKind::Endpoint);
    assert!((r.point.y - 0.03).abs() < EPS);
}
