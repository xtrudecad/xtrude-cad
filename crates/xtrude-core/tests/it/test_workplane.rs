// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The first point's work plane faces the camera in a standard view;
//! dimensions read the frontal span.

use xtrude_core::snap::{first_point_work_plane, project_to_view_plane};
use xtrude_core::DVec3;

const C: DVec3 = DVec3::new(3.0, 4.0, 5.0);
const T: f64 = 0.97;

fn fw(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

#[test]
fn test_front_view_gives_the_frontal_plane() {
    let (pt, n) = first_point_work_plane(fw(0., -1., 0.), C, T).unwrap();
    assert!(n.x == 0.0 && n.z == 0.0 && n.y.abs() == 1.0);
    assert_eq!(pt, C);
}

#[test]
fn test_side_view_gives_the_sagittal_plane() {
    let (_pt, n) = first_point_work_plane(fw(1., 0., 0.), C, T).unwrap();
    assert!(n.x.abs() == 1.0 && n.y == 0.0 && n.z == 0.0);
}

#[test]
fn test_plan_view_gives_the_horizontal_plane() {
    let (pt, n) = first_point_work_plane(fw(0., 0., -1.), C, T).unwrap();
    assert!(n.x == 0.0 && n.y == 0.0 && n.z.abs() == 1.0);
    assert_eq!(pt, C);
}

#[test]
fn test_oblique_orbit_view_returns_none() {
    assert!(first_point_work_plane(fw(1., 1., 1.), C, T).is_none());
    assert!(first_point_work_plane(fw(0.6, 0.6, 0.5), C, T).is_none());
}

#[test]
fn test_near_axis_still_counts() {
    assert!(first_point_work_plane(fw(0.05, -0.998, 0.03), C, T).is_some());
}

#[test]
fn test_degenerate_forward_is_none() {
    assert!(first_point_work_plane(DVec3::ZERO, C, T).is_none());
}

#[test]
fn test_front_view_drops_the_depth() {
    let reference = fw(0., 5., 0.);
    let out = project_to_view_plane(fw(4., 9., 3.), reference, fw(0., -1., 0.), T);
    assert_eq!(out, fw(4., 5., 3.));
    assert_eq!((out - reference).length(), 5.0);
}

#[test]
fn test_plan_view_drops_z() {
    let out = project_to_view_plane(fw(4., 5., 9.), fw(1., 1., 2.), fw(0., 0., -1.), T);
    assert_eq!(out, fw(4., 5., 2.));
}

#[test]
fn test_side_view_drops_x() {
    let out = project_to_view_plane(fw(7., 3., 4.), fw(2., 0., 0.), fw(1., 0., 0.), T);
    assert_eq!(out, fw(2., 3., 4.));
}

#[test]
fn test_oblique_view_keeps_true_3d() {
    let moving = fw(4., 9., 3.);
    assert_eq!(
        project_to_view_plane(moving, DVec3::ZERO, fw(1., 1., 1.), T),
        moving
    );
}
