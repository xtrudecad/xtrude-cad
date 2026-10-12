// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Auto-merge of coincident edges.

use crate::common;
use common::*;
use xtrude_core::geometry::Segment;
use xtrude_core::topology::{find_duplicate_edge, same_position};
use xtrude_model::commands::AddEdgeCommand;

#[test]
fn test_duplicate_same_orientation() {
    let edges = [Segment::new(V(0., 0.), V(1., 0.))];
    assert_eq!(find_duplicate_edge(&edges, V(0., 0.), V(1., 0.)), Some(0));
}

#[test]
fn test_duplicate_reversed_orientation() {
    let edges = [Segment::new(V(0., 0.), V(1., 0.))];
    assert_eq!(find_duplicate_edge(&edges, V(1., 0.), V(0., 0.)), Some(0));
}

#[test]
fn test_duplicate_within_tolerance() {
    let edges = [Segment::new(V(0., 0.), V(1., 0.))];
    assert_eq!(
        find_duplicate_edge(&edges, V(0.00001, 0.), V(1., 0.)),
        Some(0)
    );
}

#[test]
fn test_not_duplicate_beyond_tolerance() {
    let edges = [Segment::new(V(0., 0.), V(1., 0.))];
    assert_eq!(find_duplicate_edge(&edges, V(0., 0.), V(1., 0.001)), None);
}

#[test]
fn test_no_duplicate_for_different_edge() {
    let edges = [Segment::new(V(0., 0.), V(1., 0.))];
    assert_eq!(find_duplicate_edge(&edges, V(0., 1.), V(1., 1.)), None);
}

#[test]
fn test_degenerate_query_never_matches() {
    let edges = [Segment::new(V(0., 0.), V(0., 0.))];
    assert_eq!(find_duplicate_edge(&edges, V(0., 0.), V(0., 0.)), None);
}

#[test]
fn test_empty_scene() {
    assert_eq!(find_duplicate_edge(&[], V(0., 0.), V(1., 0.)), None);
}

#[test]
fn test_same_position_helper() {
    assert!(same_position(V3(1., 2., 3.), V3(1., 2., 3.)));
    assert!(same_position(V(1., 0.), V(1.00001, 0.)));
    assert!(!same_position(V(1., 0.), V(1.01, 0.)));
}

#[test]
fn test_merge_dedups_reversed_edge() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    h.run(&mut s, AddEdgeCommand::new(V(1., 0.), V(0., 0.)));
    assert_eq!(nedges(&s), 1);
}

#[test]
fn test_mesh_never_duplicates_even_with_merge_off() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    assert_eq!(nedges(&s), 1);
}

#[test]
fn test_undo_of_merged_noop_keeps_original() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    let original = s.mesh().edge_ids();
    h.run(&mut s, AddEdgeCommand::new(V(1., 0.), V(0., 0.)));
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().edge_ids(), original);
}

#[test]
fn test_undo_redo_roundtrip_for_real_add() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    let edge = s.mesh().edge_ids();
    assert!(h.undo(&mut s));
    assert!(s.mesh().edge_ids().is_empty());
    assert!(h.redo(&mut s));
    assert_eq!(s.mesh().edge_ids(), edge);
}

#[test]
fn test_undo_real_add_discards_from_selection() {
    let (mut s, mut h) = scene_history();
    let mut cmd = AddEdgeCommand::new(V(0., 0.), V(1., 0.));
    h.execute(&mut s, &mut cmd);
    let e = xtrude_model::EntityRef::Edge(s.active_mesh_id(), cmd.edge.unwrap());
    s.selection.insert(e);
    h.undo(&mut s);
    assert!(!s.selection.contains(&e));
}

#[test]
fn test_merged_command_owns_no_edge() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 0.)));
    let mut merged = AddEdgeCommand::new(V(0., 0.), V(1., 0.));
    h.execute(&mut s, &mut merged);
    assert!(merged.edge.is_none());
}

#[test]
fn test_two_rects_share_single_edge() {
    let (mut s, mut h) = scene_history();
    let rect1 = [V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)];
    let rect2 = [V(1., 0.), V(2., 0.), V(2., 1.), V(1., 1.)];
    for r in [rect1, rect2] {
        for i in 0..4 {
            h.run(&mut s, AddEdgeCommand::new(r[i], r[(i + 1) % 4]));
        }
    }
    assert_eq!(nedges(&s), 7);
    let shared = edge_keyset(&s)
        .iter()
        .filter(|k| **k == ek(V(1., 0.), V(1., 1.)))
        .count();
    assert_eq!(shared, 1);
}
