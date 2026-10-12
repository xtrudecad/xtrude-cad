// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The topology engine's basic guarantees, through the command
//! paths the tools use.

use crate::common;

use common::*;
use xtrude_core::triangulate::triangulate;
use xtrude_core::DVec3;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::edits::{build_add_edge, build_add_edges};
use xtrude_model::history::History;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;
use xtrude_model::scene::Scene;

fn face_sizes(s: &Scene) -> Vec<usize> {
    let m = s.mesh();
    let mut out: Vec<usize> = m
        .face_ids()
        .iter()
        .map(|&f| m.face(f).outer.len())
        .collect();
    out.sort();
    out
}

fn rectangle(s: &mut Scene, h: &mut History, c: [DVec3; 4]) {
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect();
    let mut cmd = build_add_edges(
        s,
        &segs,
        false,
        vec![Box::new(AddFaceCommand::new(c.to_vec()))],
        None,
    );
    h.execute(s, cmd.as_mut());
}

fn tri_area(outer: &[DVec3], holes: &[Vec<DVec3>]) -> f64 {
    triangulate(outer, holes, None)
        .iter()
        .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
        .sum()
}

fn diagonal(s: &mut Scene, h: &mut History) {
    let mut cmd = build_add_edge(s, V(0., 0.), V(2., 2.), true, None);
    h.execute(s, cmd.as_mut());
}

#[test]
fn test_diagonal_makes_two_triangles() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)]);
    diagonal(&mut s, &mut h);
    assert_eq!(face_sizes(&s), vec![3, 3]);
    assert_eq!(s.mesh().edge_ids().len(), 5);
}

#[test]
fn test_square_inside_face_divides_mother() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, [V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)]);
    rectangle(&mut s, &mut h, [V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)]);
    let m = s.mesh();
    let mother = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .expect("the mother");
    assert_eq!(m.face(mother).holes.len(), 1);
    let area = tri_area(&m.face_positions(mother), &m.face_hole_positions(mother));
    assert!((area - 12.0).abs() < 1e-6); // 16 − 4
}

#[test]
fn test_shared_edge_not_duplicated() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)]);
    rectangle(&mut s, &mut h, [V(2., 0.), V(4., 0.), V(4., 2.), V(2., 2.)]);
    assert_eq!(s.mesh().edge_ids().len(), 7); // 4 + 4 − 1 shared
}

#[test]
fn test_concave_l_fills() {
    let el = [
        V(0., 0.),
        V(2., 0.),
        V(2., 1.),
        V(1., 1.),
        V(1., 2.),
        V(0., 2.),
    ];
    assert!((tri_area(&el, &[]) - 3.0).abs() < 1e-6);
}

#[test]
fn test_pushpull_on_split_triangle() {
    let (mut s, mut h) = scene_history();
    rectangle(&mut s, &mut h, [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)]);
    diagonal(&mut s, &mut h); // → two triangles
    let triangle = s.mesh().face_ids()[0];
    assert_eq!(s.mesh().face(triangle).outer.len(), 3);
    assert!(push_face(
        &mut s,
        &mut h,
        MeshId::LOOSE,
        triangle,
        1.0,
        false
    ));
    // Prism: the base triangle plus a top face and three side quads.
    let sizes = face_sizes(&s);
    assert!(sizes.iter().filter(|&&n| n == 3).count() >= 2); // base + top
    assert!(sizes.contains(&4)); // side quads
}
