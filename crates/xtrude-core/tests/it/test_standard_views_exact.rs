// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Top and Bottom are exactly vertical.

use xtrude_core::camera::OrbitCamera;
use xtrude_core::math::MatExt;
use xtrude_core::DVec3;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn camera(view: &str, perspective: bool) -> OrbitCamera {
    let mut c = OrbitCamera::new();
    c.set_aspect(1000, 700);
    c.perspective = perspective;
    c.set_view(view);
    c
}

fn screen(c: &OrbitCamera, p: DVec3) -> (f64, f64) {
    let q = (c.projection_matrix() * c.view_matrix()).project_point(p);
    (q.x, q.y)
}

#[test]
fn test_a_vertical_edge_is_a_point_in_the_parallel_plan() {
    for view in ["top", "bottom"] {
        let c = camera(view, false);
        let (a, b) = (screen(&c, v(0., 0., 0.)), screen(&c, v(0., 0., 3.)));
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "{view}"
        );
        assert!((c.forward().z.abs() - 1.0).abs() < 1e-9, "{view}");
    }
}

#[test]
fn test_the_screen_keeps_its_orientation_from_the_89_degree_days() {
    let top = camera("top", false);
    let (o, n, e) = (
        screen(&top, v(0., 0., 0.)),
        screen(&top, v(0., 3., 0.)),
        screen(&top, v(3., 0., 0.)),
    );
    assert!(n.1 > o.1 && e.0 > o.0);
    let bottom = camera("bottom", false);
    let (o, n, e) = (
        screen(&bottom, v(0., 0., 0.)),
        screen(&bottom, v(0., 3., 0.)),
        screen(&bottom, v(3., 0., 0.)),
    );
    assert!(n.1 < o.1 && e.0 > o.0);
}

#[test]
fn test_the_other_views_and_a_rolled_up_are_untouched() {
    let c = camera("front", false);
    assert_eq!(c.up_vector(), c.up);
    let c = camera("iso", false);
    assert_eq!(c.up_vector(), c.up);
    let mut top = camera("top", false);
    top.up = v(1., 0., 0.);
    assert_eq!(top.up_vector(), v(1., 0., 0.));
}

#[test]
fn test_orbiting_out_of_the_top_view_still_works() {
    let mut c = camera("top", true);
    c.orbit(0.0, 40.0, 700);
    assert!(c.pitch.abs() < 90f64.to_radians());
    assert!(c.view_matrix().try_inverse().is_some());
}
