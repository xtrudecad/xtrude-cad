// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Move undo/redo exactness.

use crate::common;
use common::*;
use xtrude_model::commands::MoveVerticesCommand;
use xtrude_model::edits::build_add_edges;

fn positions(s: &xtrude_model::Scene) -> Vec<(i64, i64)> {
    let m = s.mesh();
    let mut v: Vec<(i64, i64)> = m
        .vertex_ids()
        .iter()
        .map(|&v| {
            (
                (m.pos(v).x * 1e4).round() as i64,
                (m.pos(v).y * 1e4).round() as i64,
            )
        })
        .collect();
    v.sort();
    v
}

fn two_squares(s: &mut xtrude_model::Scene, h: &mut xtrude_model::History) {
    let a = [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)];
    let b = [V(5., 0.), V(7., 0.), V(7., 2.), V(5., 2.)];
    for sq in [a, b] {
        let segs: Vec<_> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
        let c = build_add_edges(s, &segs, true, vec![], None);
        exec(s, h, c);
    }
}

fn move_a(dx: f64, dy: f64) -> MoveVerticesCommand {
    MoveVerticesCommand {
        src: vec![V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)],
        delta: V(dx, dy),
    }
}

#[test]
fn test_move_onto_other_vertex_then_undo_is_exact() {
    let (mut s, mut h) = scene_history();
    two_squares(&mut s, &mut h);
    let before = positions(&s);
    h.run(&mut s, move_a(3., 0.));
    h.undo(&mut s);
    assert_eq!(positions(&s), before);
}

#[test]
fn test_move_undo_redo_cycle_stable() {
    let (mut s, mut h) = scene_history();
    two_squares(&mut s, &mut h);
    let before = positions(&s);
    h.run(&mut s, move_a(3., 0.));
    let moved = positions(&s);
    h.undo(&mut s);
    h.redo(&mut s);
    assert_eq!(positions(&s), moved);
    h.undo(&mut s);
    assert_eq!(positions(&s), before);
}

#[test]
fn test_plain_move_still_works() {
    let (mut s, mut h) = scene_history();
    two_squares(&mut s, &mut h);
    h.run(&mut s, move_a(0., 10.));
    assert!(positions(&s).contains(&(0, 100000)));
    h.undo(&mut s);
    assert!(positions(&s).contains(&(0, 0)));
}
