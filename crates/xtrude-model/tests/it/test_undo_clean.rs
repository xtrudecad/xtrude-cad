// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Undo/redo of drawn geometry restores exactly.

use crate::common;
use common::*;
use xtrude_model::commands::EraseSelectionCommand;
use xtrude_model::edits::{build_add_edge, build_add_edges};

fn counts(s: &xtrude_model::Scene) -> (usize, usize, usize) {
    let m = s.mesh();
    (m.faces.len(), m.edges.len(), m.verts.len())
}

#[test]
fn test_undo_divider_leaves_no_orphans() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)], &[]);
    let before = counts(&s);
    let c = build_add_edge(&s, V(2., 0.), V(2., 4.), true, None);
    exec(&mut s, &mut h, c);
    assert!(h.undo(&mut s));
    assert_eq!(counts(&s), before);
}

#[test]
fn test_undo_hole_punch_leaves_no_orphan_vertices() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)], &[]);
    let before = counts(&s);
    let c = [V(3., 3.), V(6., 3.), V(6., 6.), V(3., 6.)];
    let segs: Vec<_> = (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&s, &segs, true, vec![], None);
    exec(&mut s, &mut h, cmd);
    assert!(h.undo(&mut s));
    assert_eq!(counts(&s), before);
}

#[test]
fn test_redo_reproduces_the_split_exactly() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)], &[]);
    let c = build_add_edge(&s, V(2., 0.), V(2., 4.), true, None);
    exec(&mut s, &mut h, c);
    let after = counts(&s);
    for _ in 0..3 {
        assert!(h.undo(&mut s));
        assert!(h.redo(&mut s));
        assert_eq!(counts(&s), after);
    }
}

#[test]
fn test_mixed_draw_erase_round_trips() {
    let (mut s, mut h) = scene_history();
    let mut states = vec![counts(&s)];
    let r = [V(0., 0.), V(6., 0.), V(6., 6.), V(0., 6.)];
    let segs: Vec<_> = (0..4).map(|i| (r[i], r[(i + 1) % 4])).collect();
    let c = build_add_edges(&s, &segs, true, vec![], None);
    exec(&mut s, &mut h, c);
    states.push(counts(&s));
    let c = build_add_edge(&s, V(3., 0.), V(3., 6.), true, None);
    exec(&mut s, &mut h, c);
    states.push(counts(&s));
    let m = s.mesh();
    let e = m
        .find_edge(
            m.vertex_at(V(3., 0.)).unwrap(),
            m.vertex_at(V(3., 6.)).unwrap(),
        )
        .unwrap();
    let cmd = EraseSelectionCommand::from_entities(m, &[e], &[]);
    h.run(&mut s, cmd);
    states.push(counts(&s));
    for i in 0..3 {
        h.undo(&mut s);
        assert_eq!(counts(&s), states[states.len() - 2 - i]);
    }
    for i in 0..3 {
        h.redo(&mut s);
        assert_eq!(counts(&s), states[i + 1]);
    }
}
