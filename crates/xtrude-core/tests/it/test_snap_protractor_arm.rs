// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — a protractor arm is a direction from the centre, not a line
//! being drawn.

use crate::snap_common;
use snap_common::*;
use xtrude_core::Axis;

fn circle(es: &mut Vec<SnapEdge>, cx: f64, r: f64, n: usize, curve: i64) {
    let pts: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            v(cx + r * a.cos(), r * a.sin(), 0.)
        })
        .collect();
    for i in 0..n {
        es.push(SnapEdge {
            soft: true,
            curve: Some(curve),
            ..edge(pts[i], pts[(i + 1) % n])
        });
    }
}

fn end() -> DVec3 {
    let t = 1.7f64.to_radians();
    v(t.cos(), t.sin(), 0.)
}

fn pacaeiro_scene() -> Vec<SnapEdge> {
    let mut es = Vec::new();
    circle(&mut es, 0.0, 1.0, 24, 1);
    es.push(edge(-end(), end()));
    circle(&mut es, 4.0, 1.0, 24, 2);
    es
}

fn arm(es: &[SnapEdge], cand: DVec3, radial_arm: bool) -> SnapResult {
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = Some(v(0., 0., 0.));
    q.project_onto_line = Some(&project);
    q.inference_angle_deg = 3.0;
    q.magnetic_axis_deg = Some(3.0);
    q.acquired_point = Some(end());
    q.radial_arm = radial_arm;
    compute_snap(&q, es)
}

#[test]
fn test_the_arm_reaches_the_red_axis_through_the_reference_window() {
    let r = arm(&pacaeiro_scene(), v(-3.0, -0.02, 0.), true);
    assert!(
        r.kind == SnapKind::Axis && r.axis == Some(Axis::X),
        "{:?}",
        r.kind
    );
    assert!(r.point.y.abs() < 1e-6);
}

#[test]
fn test_a_line_still_passes_through_the_point() {
    assert_eq!(
        arm(&pacaeiro_scene(), v(-3.0, -0.02, 0.), false).kind,
        SnapKind::ThroughPoint
    );
}

#[test]
fn test_the_arm_still_lands_on_a_point() {
    assert_eq!(
        arm(&pacaeiro_scene(), v(-1.0, -0.0297 + 0.001, 0.), true).kind,
        SnapKind::Endpoint
    );
}

#[test]
fn test_no_extension_on_the_arm() {
    let es = [edge(v(2., 0.5, 0.), v(3., 0.5, 0.))];
    let cand = v(1.0, 0.5, 0.);
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = Some(v(2., 0.5, 0.));
    q.project_onto_line = Some(&project);
    q.radial_arm = true;
    assert_ne!(compute_snap(&q, &es).kind, SnapKind::Extension);
}

fn plain_extension(e: SnapEdge) -> SnapResult {
    let cand = v(3.0, 0.05, 0.);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = Some(v(2., 0., 0.));
    compute_snap(&q, &[e])
}

#[test]
fn test_a_curve_segment_is_never_extended() {
    let e = SnapEdge {
        soft: true,
        curve: Some(7),
        ..edge(v(0., 0., 0.), v(2., 0., 0.))
    };
    assert_ne!(plain_extension(e).kind, SnapKind::Extension);
}

#[test]
fn test_a_hidden_edge_is_never_extended() {
    let e = SnapEdge {
        hidden: true,
        ..edge(v(0., 0., 0.), v(2., 0., 0.))
    };
    assert_ne!(plain_extension(e).kind, SnapKind::Extension);
}
