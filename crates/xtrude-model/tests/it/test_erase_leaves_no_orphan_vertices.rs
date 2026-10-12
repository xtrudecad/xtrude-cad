// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Erasing takes the vertices with it: an endpoint nothing else uses goes too.

use crate::common;
use common::*;
use xtrude_model::commands::EraseSelectionCommand;

fn loose_line() -> xtrude_model::Scene {
    let (mut s, _) = scene_history();
    s.mesh_mut().add_edge(V(0., 0.), V(3., 0.)).unwrap();
    s
}

fn erase(
    s: &mut xtrude_model::Scene,
    h: &mut xtrude_model::History,
    edges: &[xtrude_core::EdgeId],
    faces: &[xtrude_core::FaceId],
) {
    let c = EraseSelectionCommand::from_entities(s.mesh(), edges, faces);
    h.run(s, c);
}

#[test]
fn test_erasing_a_loose_line_leaves_nothing_behind() {
    let mut s = loose_line();
    let mut h = xtrude_model::History::new();
    let e = s.mesh().edge_ids();
    erase(&mut s, &mut h, &e, &[]);
    assert!(s.mesh().edges.is_empty());
    assert!(s.mesh().verts.is_empty(), "the endpoints must go too");
}

#[test]
fn test_a_vertex_still_used_by_a_face_survives() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    let loose = s.mesh_mut().add_edge(V(5., 5.), V(7., 5.)).unwrap();
    erase(&mut s, &mut h, &[loose], &[]);
    assert_eq!(nfaces(&s), 1);
    assert_eq!(s.mesh().verts.len(), 4);
}

#[test]
fn test_undo_brings_the_vertices_back() {
    let mut s = loose_line();
    let mut h = xtrude_model::History::new();
    let e = s.mesh().edge_ids();
    erase(&mut s, &mut h, &e, &[]);
    h.undo(&mut s);
    assert_eq!(nedges(&s), 1);
    assert_eq!(s.mesh().verts.len(), 2);
}

#[test]
fn test_the_position_registry_lets_go_as_well() {
    let mut s = loose_line();
    let mut h = xtrude_model::History::new();
    let e = s.mesh().edge_ids();
    erase(&mut s, &mut h, &e, &[]);
    let revived = s.mesh_mut().vertex(V(0., 0.));
    assert!(s.mesh().has_vertex(revived));
    assert!(s.mesh().vertex_at(V(3., 0.)).is_none());
}

#[test]
fn test_erasing_a_face_takes_its_corners() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    let e = s.mesh().edge_ids();
    let f = s.mesh().face_ids();
    erase(&mut s, &mut h, &e, &f);
    assert!(s.mesh().faces.is_empty());
    assert!(s.mesh().verts.is_empty());
}
