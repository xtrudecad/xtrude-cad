// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Command tests: a group of groups is a GROUP; Make Component
//! over several groups makes ONE component holding them; Make Independent keeps the
//! subgroups. The .igz ones are in xtrude-io, the window one in xtrude-app.

use crate::common;

use common::*;
use xtrude_core::Mesh;
use xtrude_model::commands::{
    GroupToComponentCommand, MakeComponentOfCommand, MakeIndependentCommand, MakeNestedGroupCommand,
};
use xtrude_model::group::{add_mesh, copy_group, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::scene::Scene;

fn box_group(s: &mut Scene, x: f64, name: &str) -> Group {
    let mut m = Mesh::new();
    m.add_face(&[V(x, 0.), V(x + 1., 0.), V(x + 1., 1.), V(x, 1.)], &[]);
    Group::new(add_mesh(&mut s.doc.meshes, m), Some(name))
}

fn two_groups(s: &mut Scene) -> Vec<GroupId> {
    let (a, b) = (box_group(s, 0., "A"), box_group(s, 3., "B"));
    let ids = vec![a.id, b.id];
    s.doc.groups.extend([a, b]);
    ids
}

fn child_ids(g: &Group) -> Vec<GroupId> {
    g.children.iter().map(|c| c.id).collect()
}

#[test]
fn test_a_group_of_groups_is_a_group_and_can_become_a_component() {
    let (mut s, mut h) = scene_history();
    let ids = two_groups(&mut s);
    let mut nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids.clone(), Some("Spacer"));
    h.execute(&mut s, &mut nest);
    let bx = s.doc.groups.last().unwrap();
    let id = bx.id;
    assert!(bx.xform.is_some() && !bx.is_component());
    h.execute(
        &mut s,
        &mut GroupToComponentCommand {
            group: id,
            name: Some("Spacer".into()),
        },
    );
    let bx = s.doc.find_group(id).unwrap();
    assert!(bx.is_component() && child_ids(bx) == ids);
    h.undo(&mut s);
    assert!(!s.doc.find_group(id).unwrap().is_component());
}

#[test]
fn test_make_component_over_several_groups_makes_one_holding_them() {
    let (mut s, mut h) = scene_history();
    let ids = two_groups(&mut s);
    let nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids.clone(), Some("Spacer"));
    h.execute(&mut s, &mut MakeComponentOfCommand { nest });
    assert_eq!(s.doc.groups.len(), 1);
    let comp = &s.doc.groups[0];
    assert!(comp.is_component() && child_ids(comp) == ids);
    h.undo(&mut s);
    assert_eq!(s.doc.groups.iter().map(|g| g.id).collect::<Vec<_>>(), ids);
}

#[test]
fn test_make_independent_turns_a_component_with_subgroups_into_a_group() {
    let (mut s, mut h) = scene_history();
    let ids = two_groups(&mut s);
    let nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids, Some("Spacer"));
    h.execute(&mut s, &mut MakeComponentOfCommand { nest });
    let comp_id = s.doc.groups[0].id;
    let mut comp = s.doc.groups[0].clone();
    let sibling = copy_group(&mut s.doc.meshes, &mut comp, Some(V(10., 0.)), false);
    s.doc.groups[0] = comp;
    let sib_id = sibling.id;
    assert!(sibling.is_component());
    s.doc.groups.push(sibling);
    h.execute(&mut s, &mut MakeIndependentCommand { group: comp_id });
    let comp = s.doc.find_group(comp_id).unwrap();
    assert!(!comp.is_component() && comp.children.len() == 2);
    assert!(s.doc.find_group(sib_id).unwrap().is_component());
    h.undo(&mut s);
    assert!(s.doc.find_group(comp_id).unwrap().is_component());
}
