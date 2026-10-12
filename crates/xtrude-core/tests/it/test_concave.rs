// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Concave faces fill correctly.

use xtrude_core::geometry::PolyFace;
use xtrude_core::triangulate::is_convex;
use xtrude_core::DVec3;

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn tri_area(t: &[DVec3; 3]) -> f64 {
    (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5
}

fn area(f: &PolyFace) -> f64 {
    f.triangulate().iter().map(tri_area).sum()
}

fn l_shape() -> Vec<DVec3> {
    vec![
        V(0., 0., 0.),
        V(2., 0., 0.),
        V(2., 1., 0.),
        V(1., 1., 0.),
        V(1., 2., 0.),
        V(0., 2., 0.),
    ]
}

#[test]
fn test_is_convex_square() {
    assert!(is_convex(&[(0., 0.), (2., 0.), (2., 2.), (0., 2.)]));
}

#[test]
fn test_is_convex_triangle() {
    assert!(is_convex(&[(0., 0.), (1., 0.), (0., 1.)]));
}

#[test]
fn test_is_concave_l_shape() {
    assert!(!is_convex(&[
        (0., 0.),
        (2., 0.),
        (2., 1.),
        (1., 1.),
        (1., 2.),
        (0., 2.)
    ]));
}

#[test]
fn test_is_convex_ignores_collinear_midpoint() {
    assert!(is_convex(&[
        (0., 0.),
        (1., 0.),
        (2., 0.),
        (2., 2.),
        (0., 2.)
    ]));
}

#[test]
fn test_concave_face_fills_correctly() {
    assert!((area(&PolyFace::new(l_shape())) - 3.0).abs() < 1e-6);
}

#[test]
fn test_concave_face_triangle_count() {
    let tris = PolyFace::new(l_shape()).triangulate();
    assert_eq!(tris.len(), l_shape().len() - 2);
}

#[test]
fn test_convex_face_unaffected() {
    let f = PolyFace::new(vec![
        V(0., 0., 0.),
        V(2., 0., 0.),
        V(2., 2., 0.),
        V(0., 2., 0.),
    ]);
    assert_eq!(f.triangulate().len(), 2);
    assert!((area(&f) - 4.0).abs() < 1e-6);
}

#[test]
fn test_concave_face_on_vertical_plane() {
    let el = vec![
        V(0., 0., 0.),
        V(2., 0., 0.),
        V(2., 0., 1.),
        V(1., 0., 1.),
        V(1., 0., 2.),
        V(0., 0., 2.),
    ];
    assert!((area(&PolyFace::new(el)) - 3.0).abs() < 1e-6);
}

#[test]
fn test_concave_face_rotated() {
    let ang = 40f64.to_radians();
    let (c, s) = (ang.cos(), ang.sin());
    let rot = |p: DVec3| V(p.x * c - p.y * s, p.x * s + p.y * c, 0.0);
    let f = PolyFace::new(l_shape().into_iter().map(rot).collect());
    assert!((area(&f) - 3.0).abs() < 1e-6);
}

#[test]
fn test_concave_face_with_hole_still_works() {
    let outer = vec![
        V(0., 0., 0.),
        V(4., 0., 0.),
        V(4., 1., 0.),
        V(1., 1., 0.),
        V(1., 4., 0.),
        V(0., 4., 0.),
    ];
    let hole = vec![
        V(0.25, 0.25, 0.),
        V(0.75, 0.25, 0.),
        V(0.75, 0.75, 0.),
        V(0.25, 0.75, 0.),
    ];
    let f = PolyFace::with_holes(outer, vec![hole]);
    assert!((area(&f) - (7.0 - 0.25)).abs() < 1e-6);
}
