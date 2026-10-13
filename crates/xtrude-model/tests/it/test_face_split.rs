// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hole punching: a loop drawn strictly inside a coplanar face divides it.
//! (The document round trip lives in xtrude-io.)

use crate::common;
use common::*;
use xtrude_core::geometry::PolyFace;
use xtrude_core::topology::{find_containing_face, loop_inside_face};
use xtrude_core::triangulate::triangulate;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::scene::Scene;

fn square_4() -> Vec<DVec3> {
    vec![V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)]
}

fn hole_2() -> Vec<DVec3> {
    vec![V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)]
}

fn area(tris: &[[DVec3; 3]]) -> f64 {
    tris.iter().map(tri_area).sum()
}

#[test]
fn test_triangulate_simple_square() {
    let t = triangulate(&square_4(), &[], None);
    assert_eq!(t.len(), 2);
    assert!((area(&t) - 16.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_square_with_hole_area() {
    assert!((area(&triangulate(&square_4(), &[hole_2()], None)) - 12.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_hole_on_vertical_plane() {
    let outer = [
        V3(0., 0., 0.),
        V3(4., 0., 0.),
        V3(4., 0., 4.),
        V3(0., 0., 4.),
    ];
    let hole = vec![
        V3(1., 0., 1.),
        V3(3., 0., 1.),
        V3(3., 0., 3.),
        V3(1., 0., 3.),
    ];
    assert!((area(&triangulate(&outer, &[hole], None)) - 12.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_concave_outer() {
    let el = [
        V(0., 0.),
        V(2., 0.),
        V(2., 1.),
        V(1., 1.),
        V(1., 2.),
        V(0., 2.),
    ];
    assert!((area(&triangulate(&el, &[], None)) - 3.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_hole_winding_independent() {
    let mut h = hole_2();
    h.reverse();
    assert!((area(&triangulate(&square_4(), &[h], None)) - 12.0).abs() < 1e-6);
}

#[test]
fn test_loop_inside_face_true() {
    assert!(loop_inside_face(
        &PolyFace::new(square_4()),
        &hole_2(),
        false
    ));
}

#[test]
fn test_loop_sharing_boundary_is_not_inside() {
    let touching = [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)];
    assert!(!loop_inside_face(
        &PolyFace::new(square_4()),
        &touching,
        false
    ));
}

#[test]
fn test_loop_outside_is_not_inside() {
    let outside = [V(5., 5.), V(6., 5.), V(6., 6.), V(5., 6.)];
    assert!(!loop_inside_face(
        &PolyFace::new(square_4()),
        &outside,
        false
    ));
}

#[test]
fn test_loop_not_coplanar_is_not_inside() {
    let raised = [
        V3(1., 1., 1.),
        V3(3., 1., 1.),
        V3(3., 3., 1.),
        V3(1., 3., 1.),
    ];
    assert!(!loop_inside_face(
        &PolyFace::new(square_4()),
        &raised,
        false
    ));
}

#[test]
fn test_find_containing_face_picks_mother() {
    let mut m = Mesh::new();
    let big = m.add_face(&square_4(), &[]);
    let elsewhere = m.add_face(&[V(10., 10.), V(11., 10.), V(11., 11.)], &[]);
    assert_eq!(
        find_containing_face(&m, &[big, elsewhere], &hole_2(), None),
        Some(big)
    );
}

fn mother_area(s: &Scene, f: FaceId) -> f64 {
    face_area_tris(s, f)
}

#[test]
fn test_add_inner_face_punches_mother() {
    let (mut s, mut h) = scene_history();
    h.execute(&mut s, &mut AddFaceCommand::new(square_4()));
    let mother = s.mesh().face_ids()[0];
    h.execute(&mut s, &mut AddFaceCommand::new(hole_2()));
    assert_eq!(nfaces(&s), 2);
    assert_eq!(s.mesh().face(mother).holes.len(), 1);
    assert!((mother_area(&s, mother) - 12.0).abs() < 1e-6);
}

#[test]
fn test_undo_inner_face_restores_mother() {
    let (mut s, mut h) = scene_history();
    h.execute(&mut s, &mut AddFaceCommand::new(square_4()));
    let mother = s.mesh().face_ids()[0];
    h.execute(&mut s, &mut AddFaceCommand::new(hole_2()));
    assert!(h.undo(&mut s));
    assert!(s.mesh().face(mother).holes.is_empty());
    assert_eq!(nfaces(&s), 1);
    assert!((mother_area(&s, mother) - 16.0).abs() < 1e-6);
}

#[test]
fn test_redo_inner_face_repunches() {
    let (mut s, mut h) = scene_history();
    h.execute(&mut s, &mut AddFaceCommand::new(square_4()));
    let mother = s.mesh().face_ids()[0];
    h.execute(&mut s, &mut AddFaceCommand::new(hole_2()));
    h.undo(&mut s);
    assert!(h.redo(&mut s));
    assert_eq!(s.mesh().face(mother).holes.len(), 1);
}

#[test]
fn test_outer_face_added_after_inner_is_not_punched() {
    let (mut s, mut h) = scene_history();
    h.execute(&mut s, &mut AddFaceCommand::new(hole_2()));
    h.execute(&mut s, &mut AddFaceCommand::new(square_4()));
    let big = s.mesh().face_ids()[1];
    assert_eq!(s.mesh().face(big).holes.len(), 1);
    assert!((mother_area(&s, big) - 12.0).abs() < 1e-6);
}

#[test]
fn test_small_rect_inside_big_rect_divides_mother() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, &square_4());
    rectangle(&mut s, &mut h, &hole_2());
    let m = s.mesh();
    let mother = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .unwrap();
    assert_eq!(nfaces(&s), 2);
    assert_eq!(m.face(mother).holes.len(), 1);
    assert!((mother_area(&s, mother) - 12.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_rotated_donut() {
    let a = 30f64.to_radians();
    let rot = |p: DVec3| V(p.x * a.cos() - p.y * a.sin(), p.x * a.sin() + p.y * a.cos());
    let outer: Vec<DVec3> = square_4().into_iter().map(rot).collect();
    let hole: Vec<DVec3> = hole_2().into_iter().map(rot).collect();
    assert!((area(&triangulate(&outer, &[hole], None)) - 12.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_two_aligned_holes() {
    let outer = [V(0., 0.), V(10., 0.), V(10., 5.), V(0., 5.)];
    let hole_a = vec![V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)];
    let hole_b = vec![V(5., 1.), V(7., 1.), V(7., 3.), V(5., 3.)];
    assert!((area(&triangulate(&outer, &[hole_a, hole_b], None)) - 42.0).abs() < 1e-6);
}

#[test]
fn test_triangulate_three_holes() {
    let outer = [V(0., 0.), V(10., 0.), V(10., 5.), V(0., 5.)];
    let holes = vec![
        vec![V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)],
        vec![V(5., 1.), V(7., 1.), V(7., 3.), V(5., 3.)],
        vec![V(8., 0.5), V(9., 0.5), V(9., 4.5), V(8., 4.5)],
    ];
    assert!((area(&triangulate(&outer, &holes, None)) - 38.0).abs() < 1e-6);
}

#[test]
fn test_two_rectangles_inside_face_punch_two_holes() {
    let (mut s, mut h) = scene_history();
    h.execute(
        &mut s,
        &mut AddFaceCommand::new(vec![V(0., 0.), V(10., 0.), V(10., 5.), V(0., 5.)]),
    );
    let plate = s.mesh().face_ids()[0];
    h.execute(
        &mut s,
        &mut AddFaceCommand::new(vec![V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)]),
    );
    h.execute(
        &mut s,
        &mut AddFaceCommand::new(vec![V(5., 1.), V(7., 1.), V(7., 3.), V(5., 3.)]),
    );
    assert_eq!(s.mesh().face(plate).holes.len(), 2);
    assert!((mother_area(&s, plate) - 42.0).abs() < 1e-6);
}
