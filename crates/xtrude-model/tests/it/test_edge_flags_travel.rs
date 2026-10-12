// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What an edge carries travels with it when the mesh is rebuilt: an edge
//! hidden before grouping stays hidden in the group.

use crate::common;
use common::*;
use xtrude_core::mesh::{EdgeFlags, EdgeId};
use xtrude_core::Mesh;
use xtrude_model::commands::{
    ExplodeGroupCommand, HideCommand, MakeGroupCommand, RebuildPlanarFacesCommand,
};
use xtrude_model::edits::build_add_edges;
use xtrude_model::EntityRef;

fn square_with_hidden() -> (xtrude_model::Scene, xtrude_model::History) {
    let (mut s, mut h) = scene_history();
    let sq = [V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)];
    let segs: Vec<_> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let c = build_add_edges(&s, &segs, true, vec![], None);
    exec(&mut s, &mut h, c);
    let mid = s.active_mesh_id();
    let bottom = s
        .mesh()
        .edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = s.mesh().edge_ab(e);
            a.y.abs() < 1e-9 && b.y.abs() < 1e-9
        })
        .unwrap();
    h.run(
        &mut s,
        HideCommand {
            entities: vec![EntityRef::Edge(mid, bottom)],
            hidden: true,
        },
    );
    s.mesh_mut().edge_mut(bottom).layer = Some("Axes".into());
    (s, h)
}

fn hidden_of(m: &Mesh) -> Vec<EdgeId> {
    m.edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).hidden)
        .collect()
}

fn make_group_all(
    s: &mut xtrude_model::Scene,
    h: &mut xtrude_model::History,
) -> xtrude_model::GroupId {
    let faces = s.mesh().face_ids();
    let edges = s.mesh().edge_ids();
    let mut c = MakeGroupCommand::new(s.mesh(), &faces, &edges, false, None);
    h.execute(s, &mut c);
    c.group.unwrap()
}

#[test]
fn test_grouping_does_not_revive_a_hidden_edge() {
    let (mut s, mut h) = square_with_hidden();
    let g = make_group_all(&mut s, &mut h);
    let gm = s.doc.mesh(s.find_group(g).unwrap().mesh);
    let hidden = hidden_of(gm);
    assert_eq!(hidden.len(), 1);
    assert_eq!(gm.edge(hidden[0]).layer.as_deref(), Some("Axes"));
}

#[test]
fn test_undoing_the_group_gives_it_back_hidden() {
    let (mut s, mut h) = square_with_hidden();
    let g = make_group_all(&mut s, &mut h);
    h.run(&mut s, ExplodeGroupCommand { group: g });
    let hidden = hidden_of(s.mesh());
    assert_eq!(hidden.len(), 1);
    assert_eq!(s.mesh().edge(hidden[0]).layer.as_deref(), Some("Axes"));
}

#[test]
fn test_rebuilding_faces_keeps_the_hidden_one() {
    let (mut s, mut h) = square_with_hidden();
    h.run(&mut s, RebuildPlanarFacesCommand::new());
    assert!(!hidden_of(s.mesh()).is_empty());
}

#[test]
fn test_undo_of_grouping_leaves_everything_as_it_was() {
    let (mut s, mut h) = square_with_hidden();
    make_group_all(&mut s, &mut h);
    assert!(h.undo(&mut s));
    assert_eq!(hidden_of(s.mesh()).len(), 1);
}

#[test]
fn test_the_flags_are_one_list() {
    let (s, _h) = square_with_hidden();
    let e = hidden_of(s.mesh())[0];
    let ed = s.mesh().edge(e);
    assert!(!ed.is_plain());
    // The four flags travel as one struct.
    let f = ed.flags();
    assert_eq!(
        f,
        EdgeFlags {
            soft: ed.soft,
            curve: ed.curve,
            layer: ed.layer.clone(),
            hidden: ed.hidden
        }
    );
}
