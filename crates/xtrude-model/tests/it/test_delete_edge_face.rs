// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Deleting an edge takes its faces.

use crate::common;
use common::*;
use xtrude_model::commands::DeleteEdgesCommand;
use xtrude_model::edits::build_add_edges;

fn square_with_face() -> (xtrude_model::Scene, xtrude_model::History) {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)],
    );
    (s, h)
}

#[test]
fn test_deleting_boundary_edge_removes_the_face() {
    let (mut s, mut h) = square_with_face();
    assert_eq!(nfaces(&s), 1);
    assert_eq!(nedges(&s), 4);
    let e = s.mesh().edge_ids()[0];
    let ab = s.mesh().edge_ab(e);
    h.run(
        &mut s,
        DeleteEdgesCommand {
            endpoints: vec![ab],
            cascade_faces: true,
        },
    );
    assert!(!s.mesh().has_edge(e));
    assert_eq!(nfaces(&s), 0);
    assert_eq!(nedges(&s), 3);
}

#[test]
fn test_undo_restores_edge_and_face() {
    let (mut s, mut h) = square_with_face();
    let e = s.mesh().edge_ids()[0];
    let ab = s.mesh().edge_ab(e);
    h.run(
        &mut s,
        DeleteEdgesCommand {
            endpoints: vec![ab],
            cascade_faces: true,
        },
    );
    assert!(h.undo(&mut s));
    assert!(s.mesh().has_edge(e));
    assert_eq!(nfaces(&s), 1);
    assert_eq!(nedges(&s), 4);
}

#[test]
fn test_edge_split_keeps_the_face() {
    let (mut s, mut h) = square_with_face();
    let c = build_add_edges(&s, &[(V(1., 0.), V(1., 1.))], false, vec![], None);
    exec(&mut s, &mut h, c);
    assert_eq!(nfaces(&s), 1);
}
