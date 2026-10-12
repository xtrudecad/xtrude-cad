// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — inside a turned group the drawing axes are the group's.
//! (The viewport tests live with the app; the group-making ones
//! in xtrude-model.)

use xtrude_core::axes;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::snap::detect_axis_alignment;
use xtrude_core::Axis;
use xtrude_core::DVec3;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-4
}

fn turn(deg: f64) -> DMat4 {
    DMat4::from_translation(v(2., 1.)) * rotation_deg(DVec3::Z, deg)
}

#[test]
fn test_sync_turns_every_table_at_once_and_back() {
    axes::sync(Some(&turn(30.0)));
    let a = 30f64.to_radians();
    assert!(close(axes::axis(Axis::X), v(a.cos(), a.sin())));
    assert!(close(axes::origin(), v(2., 1.)));
    axes::sync(None);
    assert_eq!(axes::axis(Axis::X), DVec3::X);
    assert_eq!(axes::origin(), DVec3::ZERO);
}

#[test]
fn test_axis_alignment_is_measured_on_the_local_axes() {
    let a = 30f64.to_radians();
    let along_red = v(a.cos(), a.sin());
    axes::sync(None);
    assert_eq!(detect_axis_alignment(v(0., 0.), along_red, 3.0), None);
    axes::sync(Some(&turn(30.0)));
    assert_eq!(
        detect_axis_alignment(v(0., 0.), along_red, 3.0),
        Some(Axis::X)
    );
    axes::sync(None);
}

#[test]
fn test_a_rectangle_lines_up_with_the_group() {
    axes::sync(Some(&turn(30.0)));
    let (u, _v) = axes::plane_axes(DVec3::Z);
    let a = 30f64.to_radians();
    assert!(close(u, v(a.cos(), a.sin())));
    axes::sync(None);
}
