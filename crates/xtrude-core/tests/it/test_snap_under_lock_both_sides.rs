// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — Shift over the soft axis cue and the reference-edge lock
//! offer the same snaps as the arrow lock. (The viewport/camera sweeps live
//! with the app.)

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::RefMode;
use xtrude_core::Axis;

fn box_edges() -> Vec<SnapEdge> {
    vec![
        edge(v(2., -1., 0.), v(6., -1., 0.)),
        edge(v(6., -1., 0.), v(6., 3., 0.)),
    ]
}

fn shift_soft(cand: DVec3, edges: &[SnapEdge]) -> SnapResult {
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p_up);
    q.start_point = Some(v(0., 0., 0.));
    q.project_onto_line = Some(&project);
    q.shift_held = true;
    compute_snap(&q, edges)
}

#[test]
fn test_shift_over_the_soft_axis_cue_finds_the_crossing() {
    assert_eq!(
        shift_soft(v(6.0, 0.03, 0.0), &box_edges()).kind,
        SnapKind::Intersection
    );
}

#[test]
fn test_shift_over_the_soft_axis_cue_lines_up_with_a_hovered_corner() {
    let corner = v(6., -1., 0.);
    let cand = v(6., 0.02, 0.);
    let project = proj_factory(cand);
    let mut q = query_with(cand, &w2p_up);
    q.candidate_pixel = w2p_up(corner).unwrap();
    q.start_point = Some(v(0., 0., 0.));
    q.project_onto_line = Some(&project);
    q.shift_held = true;
    let r = compute_snap(&q, &box_edges()[..1]);
    assert_eq!(r.kind, SnapKind::FromPoint);
    assert!((r.point - v(6., 0., 0.)).length() < 1e-6);
    assert!((r.guide.unwrap().0 - corner).length() < 1e-6);
}

#[test]
fn test_shift_over_the_soft_axis_cue_lands_on_a_vertex_on_the_line() {
    let r = shift_soft(v(4.02, 0.01, 0.0), &[edge(v(4., 0., 0.), v(4., 2., 0.))]);
    assert_eq!(r.kind, SnapKind::Endpoint);
}

#[test]
fn test_shift_over_the_soft_axis_cue_still_locks_with_nothing_around() {
    let r = shift_soft(v(3.0, 0.05, 0.0), &[]);
    assert!(r.kind == SnapKind::Axis && r.axis == Some(Axis::X));
    assert!(r.point.y.abs() < 1e-9);
}

fn reference_lock(
    mode: RefMode,
    cand: DVec3,
    edges: &[SnapEdge],
    cursor: Option<DVec3>,
    reference: Option<SnapEdge>,
) -> SnapResult {
    let reference = reference.unwrap_or_else(|| edge(v(-2., 0., 0.), v(2., 0., 0.)));
    let project = move |s: DVec3, d: DVec3| s + d * (cand - s).dot(d);
    let mut all = vec![reference.clone()];
    all.extend_from_slice(edges);
    let mut q = query_with(cand, &w2p_up);
    q.candidate_pixel = w2p_up(cursor.unwrap_or(cand)).unwrap();
    q.start_point = Some(v(0., 0., 0.));
    q.project_onto_line = Some(&project);
    q.reference_edge = Some((reference.a, reference.b));
    q.reference_mode = Some(mode);
    compute_snap(&q, &all)
}

#[test]
fn test_reference_edge_lock_snaps_to_crossings() {
    for (mode, cand, crossing, es) in [
        (
            RefMode::Parallel,
            v(3.03, 0.04, 0.),
            v(3., 0., 0.),
            vec![edge(v(3., -1., 0.), v(3., 1., 0.))],
        ),
        (
            RefMode::Perpendicular,
            v(0.04, 3.03, 0.),
            v(0., 3., 0.),
            vec![edge(v(-1., 3., 0.), v(1., 3., 0.))],
        ),
    ] {
        let r = reference_lock(mode, cand, &es, None, None);
        assert_eq!(r.kind, SnapKind::Intersection, "{mode:?}");
        assert!((r.point - crossing).length() < 1e-6);
    }
}

#[test]
fn test_reference_edge_lock_aligns_with_an_off_line_corner() {
    for (mode, cand, corner, foot) in [
        (
            RefMode::Parallel,
            v(3., 0.02, 0.),
            v(3., 1., 0.),
            v(3., 0., 0.),
        ),
        (
            RefMode::Perpendicular,
            v(0.02, 3., 0.),
            v(1., 3., 0.),
            v(0., 3., 0.),
        ),
    ] {
        let es = [edge(corner, corner + v(1., 0., 0.))];
        let r = reference_lock(mode, cand, &es, Some(corner), None);
        assert_eq!(r.kind, SnapKind::FromPoint, "{mode:?}");
        assert!((r.point - foot).length() < 1e-6);
    }
}

#[test]
fn test_reference_edge_lock_keeps_its_direction_without_a_snap() {
    for (mode, cand, locked) in [
        (RefMode::Parallel, v(-3., 0.05, 0.), v(-3., 0., 0.)),
        (RefMode::Perpendicular, v(0.05, -3., 0.), v(0., -3., 0.)),
    ] {
        let r = reference_lock(mode, cand, &[], None, None);
        assert_eq!(r.kind, SnapKind::Reference, "{mode:?}");
        assert!((r.point - locked).length() < 1e-6);
    }
}

#[test]
fn test_reference_edge_lock_aligns_points_to_diagonal_vectors() {
    for (mode, cand, corner, foot) in [
        (
            RefMode::Parallel,
            v(3.01, 3.01, 0.),
            v(2., 4., 0.),
            v(3., 3., 0.),
        ),
        (
            RefMode::Perpendicular,
            v(-3.01, 3.01, 0.),
            v(-4., 2., 0.),
            v(-3., 3., 0.),
        ),
    ] {
        let reference = edge(v(0., 0., 0.), v(1., 1., 0.));
        let es = [edge(corner, corner + v(1., 0., 0.))];
        let r = reference_lock(mode, cand, &es, Some(corner), Some(reference));
        assert_eq!(r.kind, SnapKind::FromPoint, "{mode:?}");
        assert!((r.point - foot).length() < 1e-6);
    }
}
