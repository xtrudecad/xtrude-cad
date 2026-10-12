// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Face subdivision by a loop drawn against the boundary.

use crate::common;
use common::*;
use xtrude_core::geometry::PolyFace;
use xtrude_core::topology::{find_subdividing_chain, subtract_loop_from_face};
use xtrude_core::DVec3;
use xtrude_model::commands::AddFaceCommand;

fn mother() -> Vec<DVec3> {
    vec![V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)]
}
fn corner() -> Vec<DVec3> {
    vec![V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)]
}
fn partial() -> Vec<DVec3> {
    vec![V(2., 0.), V(6., 0.), V(6., 4.), V(2., 4.)]
}
fn inside() -> Vec<DVec3> {
    vec![V(2., 2.), V(4., 2.), V(4., 4.), V(2., 4.)]
}

#[test]
fn test_subdivide_face_that_already_has_a_hole() {
    let (mut s, mut h) = scene_history();
    let plate = vec![V(0., 0.), V(4., 0.), V(4., 3.), V(0., 3.)];
    h.run(&mut s, AddFaceCommand::new(plate));
    let plate_face = s.mesh().face_ids()[0];
    let hole = vec![V(2.5, 1.5), V(3.5, 1.5), V(3.5, 2.5), V(2.5, 2.5)];
    h.run(&mut s, AddFaceCommand::new(hole.clone()));
    assert_eq!(s.mesh().face(plate_face).holes.len(), 1);
    let notch = vec![V(1., 0.), V(2., 0.), V(2., 2.), V(1., 2.)];
    h.run(&mut s, AddFaceCommand::new(notch.clone()));
    let m = s.mesh();
    let notch_face = m
        .face_ids()
        .into_iter()
        .find(|&f| key_set(&m.face_positions(f)) == key_set(&notch))
        .expect("notch is its own face");
    assert!(m.face(notch_face).holes.is_empty());
    let remainder = m
        .face_ids()
        .into_iter()
        .find(|&f| f != notch_face && key_set(&m.face_positions(f)) != key_set(&hole))
        .unwrap();
    assert_eq!(m.face(remainder).outer.len(), 8);
    assert_eq!(m.face(remainder).holes.len(), 1);
    assert!(h.undo(&mut s));
    let m = s.mesh();
    assert!(!m
        .face_ids()
        .into_iter()
        .any(|f| key_set(&m.face_positions(f)) == key_set(&notch)));
}

#[test]
fn test_subdividing_chain_corner() {
    let chain = find_subdividing_chain(&PolyFace::new(mother()), &corner()).unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain
        .iter()
        .any(|v| (v.x - 4.0).abs() < 1e-9 && (v.y - 4.0).abs() < 1e-9));
}

#[test]
fn test_subdividing_chain_strictly_inside_is_none() {
    assert!(find_subdividing_chain(&PolyFace::new(mother()), &inside()).is_none());
}

#[test]
fn test_subtract_corner_gives_hexagon() {
    let r = subtract_loop_from_face(&PolyFace::new(mother()), &corner()).unwrap();
    assert_eq!(r.len(), 6);
}

#[test]
fn test_subtract_partial_edge_gives_octagon() {
    let r = subtract_loop_from_face(&PolyFace::new(mother()), &partial()).unwrap();
    assert_eq!(r.len(), 8);
}

#[test]
fn test_subtract_inside_is_none() {
    assert!(subtract_loop_from_face(&PolyFace::new(mother()), &inside()).is_none());
}

fn total_area(s: &xtrude_model::Scene) -> f64 {
    s.mesh()
        .face_ids()
        .into_iter()
        .map(|f| face_area_tris(s, f))
        .sum()
}

#[test]
fn test_corner_rect_subdivides_mother() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddFaceCommand::new(mother()));
    rectangle(&mut s, &mut h, &corner());
    assert_eq!(vcount(&s), vec![4, 6]);
    assert!((total_area(&s) - 100.0).abs() < 1e-6);
}

#[test]
fn test_corner_rect_undo_restores_mother() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddFaceCommand::new(mother()));
    rectangle(&mut s, &mut h, &corner());
    assert!(h.undo(&mut s));
    assert_eq!(vcount(&s), vec![4]);
    assert!((total_area(&s) - 100.0).abs() < 1e-6);
}

#[test]
fn test_partial_edge_rect_subdivides() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddFaceCommand::new(mother()));
    rectangle(&mut s, &mut h, &partial());
    assert_eq!(vcount(&s), vec![4, 8]);
    let mut areas: Vec<f64> = s
        .mesh()
        .face_ids()
        .into_iter()
        .map(|f| face_area_tris(&s, f))
        .collect();
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!((areas[0] - 16.0).abs() < 1e-6 && (areas[1] - 84.0).abs() < 1e-6);
}

#[test]
fn test_strictly_inside_still_punches_hole() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddFaceCommand::new(mother()));
    rectangle(&mut s, &mut h, &inside());
    let m = s.mesh();
    let mother = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .unwrap();
    assert_eq!(m.face(mother).holes.len(), 1);
    assert_eq!(m.faces.len(), 2);
}
