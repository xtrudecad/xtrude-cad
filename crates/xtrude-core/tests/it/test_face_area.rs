// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Face.area().

use xtrude_core::geometry::PolyFace;
use xtrude_core::DVec3;

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

#[test]
fn test_area_axis_aligned_rectangle() {
    let plate = PolyFace::new(vec![
        V(0., 0., 0.),
        V(4., 0., 0.),
        V(4., 0., 3.),
        V(0., 0., 3.),
    ]);
    assert_eq!(plate.area(), 12.0);
}

#[test]
fn test_area_triangle() {
    let tri = PolyFace::new(vec![V(0., 0., 0.), V(2., 0., 0.), V(0., 0., 2.)]);
    assert_eq!(tri.area(), 2.0);
}

#[test]
fn test_area_independent_of_winding() {
    let cw = PolyFace::new(vec![
        V(0., 0., 0.),
        V(2., 0., 0.),
        V(2., 2., 0.),
        V(0., 2., 0.),
    ]);
    let ccw = PolyFace::new(vec![
        V(0., 0., 0.),
        V(0., 2., 0.),
        V(2., 2., 0.),
        V(2., 0., 0.),
    ]);
    assert_eq!(cw.area(), 4.0);
    assert_eq!(ccw.area(), 4.0);
}

#[test]
fn test_degenerate_face_has_zero_area() {
    assert_eq!(
        PolyFace::new(vec![V(0., 0., 0.), V(1., 0., 0.)]).area(),
        0.0
    );
    assert_eq!(PolyFace::new(vec![]).area(), 0.0);
}

#[test]
fn test_inner_face_is_smaller_than_mother() {
    let plate = PolyFace::new(vec![
        V(0., 0., 0.),
        V(4., 0., 0.),
        V(4., 0., 3.),
        V(0., 0., 3.),
    ]);
    let notch = PolyFace::new(vec![
        V(1., 0., 0.),
        V(2., 0., 0.),
        V(2., 0., 2.),
        V(1., 0., 2.),
    ]);
    assert!(notch.area() < plate.area());
}
