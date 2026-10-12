// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — the axis under the cursor measured in pixels.
//! (Which tools ask for it is checked with the app tools.)

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::{compute_snap_with_reach, detect_axis_on_screen, MAX_AXIS_REACH};
use xtrude_core::Axis;

fn top_view(p: DVec3) -> Option<(f64, f64)> {
    Some((400.0 + p.x * 50.0, 300.0 - p.y * 50.0))
}

fn front_view(p: DVec3) -> Option<(f64, f64)> {
    Some((400.0 + p.x * 50.0, 300.0 - p.z * 50.0))
}

fn iso(p: DVec3) -> Option<(f64, f64)> {
    Some((
        400.0 + (p.x - p.y) * 35.0,
        300.0 - (p.z * 40.0 + (p.x + p.y) * 18.0),
    ))
}

const START: DVec3 = DVec3::ZERO;

fn detect(
    cand: DVec3,
    view: fn(DVec3) -> Option<(f64, f64)>,
    px_of: DVec3,
    thr: f64,
) -> Option<Axis> {
    detect_axis_on_screen(START, cand, view(px_of).unwrap(), &view, thr)
}

#[test]
fn test_the_axis_under_the_cursor_is_found() {
    let cand = v(0., 0., 2.);
    assert_eq!(detect(cand, front_view, cand, 9.0), Some(Axis::Z));
}

#[test]
fn test_all_three_are_reachable_from_an_oblique() {
    let found: std::collections::BTreeSet<Option<Axis>> =
        [v(2., 0., 0.), v(0., 2., 0.), v(0., 0., 2.)]
            .iter()
            .map(|&c| detect(c, iso, c, 9.0))
            .collect();
    assert_eq!(
        found,
        [Some(Axis::X), Some(Axis::Y), Some(Axis::Z)]
            .into_iter()
            .collect()
    );
}

#[test]
fn test_a_cursor_between_the_axes_gets_none() {
    let cand = v(2.0, 0.5, 0.0);
    assert_eq!(detect(cand, iso, cand, 9.0), None);
}

#[test]
fn test_what_you_point_at_wins_even_if_the_world_disagrees() {
    let diagonal = v(2.0, 2.0, 0.0);
    assert_eq!(detect(diagonal, iso, diagonal, 9.0), Some(Axis::Z));
}

#[test]
fn test_an_axis_pointing_at_the_camera_is_skipped() {
    assert_ne!(detect(v(0., 0., 2.), top_view, START, 9.0), Some(Axis::Z));
}

#[test]
fn test_the_nearest_axis_wins_when_two_are_close() {
    let cand = v(2.0, 0.12, 0.0);
    assert_eq!(detect(cand, top_view, cand, 40.0), Some(Axis::X));
}

#[test]
fn test_no_reach_no_answer() {
    assert_eq!(detect(START, iso, START, 9.0), None);
}

#[test]
fn test_it_reads_the_line_both_ways() {
    let cand = v(-2.0, 0.0, 0.0);
    assert_eq!(detect(cand, top_view, cand, 9.0), Some(Axis::X));
}

#[test]
fn test_the_threshold_is_in_pixels_so_it_widens_as_you_draw_short() {
    let (near, far) = (v(0.6, 0.06, 0.0), v(3.0, 0.06, 0.0));
    assert!(0.06f64.atan2(0.6) > 0.06f64.atan2(3.0));
    assert_eq!(detect(near, top_view, near, 9.0), Some(Axis::X));
    assert_eq!(detect(far, top_view, far, 9.0), Some(Axis::X));
}

/// MoveTool.magnetic_axis_deg / screen_axis_px.
const MOVE_MAGNETIC_AXIS_DEG: f64 = 15.0;
const MOVE_SCREEN_AXIS_PX: f64 = 9.0;

#[test]
fn test_move_finds_x_and_y_from_an_oblique_view_through_the_screen() {
    let cand = v(1.9, -0.4, 0.05);
    let project = proj_factory(cand);
    let mut q = SnapQuery::new(cand, iso(v(2., 0., 0.)).unwrap(), &iso, 9.0);
    q.edge_threshold_px = Some(14.0);
    q.start_point = Some(START);
    q.project_onto_line = Some(&project);
    q.magnetic_axis_deg = Some(MOVE_MAGNETIC_AXIS_DEG);
    q.screen_axis_px = Some(MOVE_SCREEN_AXIS_PX);
    let r = compute_snap(&q, &[]);
    assert!(r.kind == SnapKind::Axis && r.axis == Some(Axis::X), "{r:?}");
    assert!(r.point.y.abs() < 1e-6 && r.point.z.abs() < 1e-6);
}

#[test]
fn test_the_point_may_not_run_to_the_other_side_of_the_county() {
    let cand = v(0.0, 0.5, 0.0);
    let nearly_end_on = |p: DVec3| Some((400.0 + p.x * 60.0, 300.0 - p.y * 60.0 - p.z * 14.0));
    let very_far = |s: DVec3, d: DVec3| s + d * 300.0;
    let ask = |cap: f64| {
        let mut q = SnapQuery::new(cand, nearly_end_on(cand).unwrap(), &nearly_end_on, 9.0);
        q.edge_threshold_px = Some(14.0);
        q.start_point = Some(START);
        q.screen_axis_px = Some(9.0);
        q.project_onto_line = Some(&very_far);
        compute_snap_with_reach(&q, &[], cap)
    };
    let loose = ask(1e9);
    assert_eq!(loose.kind, SnapKind::Axis);
    assert!(
        (loose.point - START).length() > 200.0,
        "without a cap it runs away"
    );
    let capped = ask(5.0);
    assert_ne!(
        capped.kind,
        SnapKind::Axis,
        "the cap must drop the inference"
    );
    assert!((capped.point - START).length() < 1.0);
}

#[test]
fn test_the_cap_is_loose_enough_for_the_real_cases() {
    const { assert!(MAX_AXIS_REACH > 4.01) };
}
