// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A face the user reversed stays reversed while drawing elsewhere or across
//! it.

use crate::common;
use common::*;
use xtrude_core::topology::point_in_face_solid;
use xtrude_core::{DVec3, FaceId};
use xtrude_model::commands::{AddFaceCommand, EraseSelectionCommand, FlipFacesCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::{Command, History};
use xtrude_model::scene::Scene;

fn rect(s: &mut Scene, h: &mut History, x0: f64, y0: f64, x1: f64, y1: f64) {
    let c = [V(x0, y0), V(x1, y0), V(x1, y1), V(x0, y1)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect();
    let face: Box<dyn Command> = Box::new(AddFaceCommand::new(c.to_vec()));
    let mut cmd = build_add_edges(s, &segs, false, vec![face], None);
    h.execute(s, &mut *cmd);
}

fn face_at(s: &Scene, x: f64, y: f64) -> FaceId {
    let m = s.mesh();
    let hits: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| point_in_face_solid(&m.poly_face(f), V(x, y)))
        .collect();
    assert_eq!(hits.len(), 1, "{} faces at ({x}, {y})", hits.len());
    hits[0]
}

fn is_up(s: &Scene, x: f64, y: f64) -> bool {
    s.mesh().face_normal(face_at(s, x, y)).z > 0.0
}

fn reverse(s: &mut Scene, h: &mut History, x: f64, y: f64) {
    let f = face_at(s, x, y);
    let mid = s.active_mesh_id();
    h.execute(
        s,
        &mut FlipFacesCommand {
            faces: vec![(mid, f)],
        },
    );
}

fn two_with_b_reversed() -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    rect(&mut s, &mut h, 0., 0., 4., 4.);
    rect(&mut s, &mut h, 6., 0., 9., 3.);
    assert!(is_up(&s, 2., 2.) && is_up(&s, 7.5, 1.5));
    reverse(&mut s, &mut h, 7.5, 1.5);
    assert!(!is_up(&s, 7.5, 1.5));
    (s, h)
}

fn lines(s: &mut Scene, h: &mut History, segs: &[(DVec3, DVec3)]) {
    let mut cmd = build_add_edges(s, segs, true, vec![], None);
    h.execute(s, &mut *cmd);
}

#[test]
fn test_a_loose_rectangle_does_not_turn_a_reversed_face_back() {
    let (mut s, mut h) = two_with_b_reversed();
    rect(&mut s, &mut h, 12., 0., 14., 2.);
    assert!(!is_up(&s, 7.5, 1.5), "B was turned back by a far rectangle");
    assert!(is_up(&s, 2., 2.));
    assert!(
        is_up(&s, 13., 1.),
        "a region drawn anew follows the majority"
    );
}

#[test]
fn test_a_loose_arc_does_not_turn_a_reversed_face_back() {
    let (mut s, mut h) = two_with_b_reversed();
    let arc: Vec<DVec3> = (0..13)
        .map(|a| {
            let t = a as f64 * std::f64::consts::PI / 12.0;
            V(11. + t.cos(), 5. + t.sin())
        })
        .collect();
    let segs: Vec<(DVec3, DVec3)> = arc.windows(2).map(|w| (w[0], w[1])).collect();
    lines(&mut s, &mut h, &segs);
    assert!(!is_up(&s, 7.5, 1.5));
    assert!(is_up(&s, 2., 2.));
}

#[test]
fn test_a_rectangle_over_a_reversed_face_takes_its_side_there() {
    let (mut s, mut h) = two_with_b_reversed();
    rect(&mut s, &mut h, 5., 2., 8., 6.);
    assert!(!is_up(&s, 7.5, 1.), "what is left of B");
    assert!(!is_up(&s, 7., 2.5), "the overlap, cut out of B");
    assert!(is_up(&s, 6.5, 4.5), "the part on bare ground");
}

#[test]
fn test_a_line_across_a_reversed_face_keeps_both_halves_reversed() {
    let (mut s, mut h) = two_with_b_reversed();
    lines(&mut s, &mut h, &[(V(7.5, 0.), V(7.5, 3.))]);
    assert!(!is_up(&s, 7., 1.5));
    assert!(!is_up(&s, 8., 1.5));
}

#[test]
fn test_erasing_the_split_line_keeps_the_reunited_face_reversed() {
    let (mut s, mut h) = two_with_b_reversed();
    lines(&mut s, &mut h, &[(V(7.5, 0.), V(7.5, 3.))]);
    let m = s.mesh();
    let line = m
        .edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.x - 7.5).abs() < 1e-6 && (b.x - 7.5).abs() < 1e-6
        })
        .unwrap();
    let mut cmd = EraseSelectionCommand::from_entities(m, &[line], &[]);
    h.execute(&mut s, &mut cmd);
    assert!(!is_up(&s, 7.5, 1.5));
}

#[test]
fn test_undo_still_restores_the_face_as_it_was() {
    let (mut s, mut h) = two_with_b_reversed();
    rect(&mut s, &mut h, 12., 0., 14., 2.);
    h.undo(&mut s);
    assert!(!is_up(&s, 7.5, 1.5));
    h.undo(&mut s);
    assert!(is_up(&s, 7.5, 1.5));
}
