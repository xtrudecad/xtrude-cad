// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Core tests: the group tree behind the model tree panel and the reparent command (things
//! stay where they are in the world). The panel half is in xtrude-app.

use crate::common;

use common::*;
use std::collections::BTreeMap;
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::Mesh;
use xtrude_model::commands::{
    GroupToComponentCommand, MakeNestedGroupCommand, ReparentGroupsCommand,
};
use xtrude_model::group::{add_mesh, iter_placements, Group};
use xtrude_model::history::History;
use xtrude_model::ids::GroupId;
use xtrude_model::model_tree;
use xtrude_model::scene::Scene;

fn box_group(s: &mut Scene, x: f64, name: &str) -> Group {
    let mut m = Mesh::new();
    m.add_face(&[V(x, 0.), V(x + 1., 0.), V(x + 1., 1.), V(x, 1.)], &[]);
    Group::new(add_mesh(&mut s.doc.meshes, m), Some(name))
}

fn scene(names: &[&str]) -> (Scene, History, Vec<GroupId>) {
    let mut s = Scene::new();
    let mut ids = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let g = box_group(&mut s, 3.0 * i as f64, n);
        ids.push(g.id);
        s.doc.groups.push(g);
    }
    (s, History::new(), ids)
}

/// `name → sorted world vertex positions` over the whole tree.
fn world_points(s: &Scene) -> BTreeMap<String, Vec<(i64, i64, i64)>> {
    let mut out = BTreeMap::new();
    for top in &s.doc.groups {
        for (g, m) in iter_placements(top, None) {
            let mut pts: Vec<(i64, i64, i64)> = s
                .doc
                .mesh(g.mesh)
                .verts
                .iter()
                .map(|(_, v)| {
                    let p = m.map_or(v.position, |m| m.project_point(v.position));
                    let r = |x: f64| (x * 1e6).round() as i64;
                    (r(p.x), r(p.y), r(p.z))
                })
                .collect();
            pts.sort();
            out.insert(g.name.clone(), pts);
        }
    }
    out
}

fn nest(s: &mut Scene, h: &mut History, ids: Vec<GroupId>, name: &str) -> GroupId {
    let mut c = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids, Some(name));
    h.execute(s, &mut c);
    s.doc.groups.last().unwrap().id
}

fn top_ids(s: &Scene) -> Vec<GroupId> {
    s.doc.groups.iter().map(|g| g.id).collect()
}

fn kids(s: &Scene, g: GroupId) -> Vec<GroupId> {
    s.doc
        .find_group(g)
        .unwrap()
        .children
        .iter()
        .map(|c| c.id)
        .collect()
}

fn reparent(
    s: &mut Scene,
    h: &mut History,
    groups: Vec<GroupId>,
    parent: Option<GroupId>,
    index: Option<usize>,
) {
    h.execute(
        s,
        &mut ReparentGroupsCommand {
            groups,
            parent,
            index,
        },
    );
}

#[test]
fn test_parent_map_and_ancestors_walk_the_nesting() {
    let (mut s, mut h, ids) = scene(&["A", "B", "C"]);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let inner = nest(&mut s, &mut h, vec![a, b], "Inner");
    let outer = nest(&mut s, &mut h, vec![inner, c], "Outer");
    let pm = model_tree::parent_map(&s);
    assert_eq!(model_tree::ancestors(&pm, a), vec![outer, inner]);
    assert!(model_tree::ancestors(&pm, outer).is_empty());
    assert_eq!(pm[&c], Some(outer));
}

#[test]
fn test_reparenting_into_a_plain_group_keeps_everything_in_place() {
    let (mut s, mut h, ids) = scene(&["A", "B"]);
    let (a, b) = (ids[0], ids[1]);
    let before = world_points(&s);
    reparent(&mut s, &mut h, vec![a], Some(b), None);
    assert_eq!(top_ids(&s), vec![b]);
    assert_eq!(kids(&s, b), vec![a]);
    let gb = s.doc.find_group(b).unwrap();
    assert!(gb.xform.is_some() && !gb.is_component()); // a GROUP
    assert_eq!(world_points(&s), before);
    h.undo(&mut s);
    assert_eq!(top_ids(&s), vec![a, b]);
    assert!(kids(&s, b).is_empty());
    assert!(s.doc.find_group(b).unwrap().xform.is_none());
    assert!(s.doc.find_group(a).unwrap().xform.is_none());
    assert_eq!(world_points(&s), before);
}

#[test]
fn test_reparenting_into_a_transformed_group_compensates_its_matrix() {
    let (mut s, mut h, ids) = scene(&["A", "B"]);
    let (a, b) = (ids[0], ids[1]);
    let p = nest(&mut s, &mut h, vec![b], "P");
    let mv = DMat4::from_translation(V3(5., 7., 0.)) * DMat4::from_rotation_z(90f64.to_radians());
    let g = s.doc.find_group_mut(p).unwrap();
    g.xform = Some(mv * g.xform.unwrap_or(DMat4::IDENTITY));
    let before = world_points(&s);
    reparent(&mut s, &mut h, vec![a], Some(p), None);
    assert!(kids(&s, p).contains(&a));
    let ga = s.doc.find_group(a).unwrap();
    assert!(ga.xform.is_some() && !ga.is_component());
    assert_eq!(world_points(&s), before);
    h.undo(&mut s);
    assert!(top_ids(&s).contains(&a) && s.doc.find_group(a).unwrap().xform.is_none());
    assert_eq!(world_points(&s), before);
}

#[test]
fn test_moving_a_nested_group_out_to_the_model_keeps_its_place() {
    let (mut s, mut h, ids) = scene(&["A", "B"]);
    let (a, b) = (ids[0], ids[1]);
    let p = nest(&mut s, &mut h, vec![a, b], "P");
    let g = s.doc.find_group_mut(p).unwrap();
    g.xform = Some(DMat4::from_translation(V3(0., 0., 4.)) * g.xform.unwrap_or(DMat4::IDENTITY));
    let before = world_points(&s);
    reparent(&mut s, &mut h, vec![a], None, None);
    assert_eq!(kids(&s, p), vec![b]);
    assert!(top_ids(&s).contains(&a));
    assert_eq!(world_points(&s), before);
    h.undo(&mut s);
    assert_eq!(kids(&s, p), vec![a, b]);
    assert!(!top_ids(&s).contains(&a));
    assert_eq!(world_points(&s), before);
}

#[test]
fn test_several_groups_move_in_one_undo_step_at_an_index() {
    let (mut s, mut h, ids) = scene(&["A", "B", "C", "D"]);
    let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);
    let p = nest(&mut s, &mut h, vec![d], "P");
    reparent(&mut s, &mut h, vec![a, c], Some(p), Some(0));
    assert_eq!(kids(&s, p), vec![a, c, d]);
    assert_eq!(top_ids(&s), vec![b, p]);
    h.undo(&mut s);
    assert_eq!(top_ids(&s), vec![a, b, c, p]);
    assert_eq!(kids(&s, p), vec![d]);
}

#[test]
fn test_no_group_goes_inside_itself_or_in_or_out_of_a_component() {
    let (mut s, mut h, ids) = scene(&["A", "B", "C"]);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let p = nest(&mut s, &mut h, vec![a], "P");
    assert!(!model_tree::can_reparent(&s, &[p], Some(p), false));
    assert!(!model_tree::can_reparent(&s, &[p], Some(a), false)); // its own child
    assert!(model_tree::can_reparent(&s, &[b], Some(p), false));
    assert!(model_tree::can_reparent(&s, &[a], None, false));
    h.execute(
        &mut s,
        &mut GroupToComponentCommand {
            group: p,
            name: Some("Comp".into()),
        },
    );
    // A component's insides are its definition, shared by every copy.
    assert!(!model_tree::can_reparent(&s, &[b], Some(p), false));
    assert!(!model_tree::can_reparent(&s, &[a], None, false));
    assert!(model_tree::can_reparent(&s, &[b], Some(c), false));
}

#[test]
fn test_nothing_reparents_while_a_group_is_open() {
    let (mut s, _h, ids) = scene(&["A", "B"]);
    let (a, b) = (ids[0], ids[1]);
    s.begin_group_edit(b);
    assert!(!model_tree::can_reparent(&s, &[a], Some(b), false));
    assert!(model_tree::can_reparent(&s, &[a], Some(b), true));
    s.end_group_edit();
    assert!(model_tree::can_reparent(&s, &[a], Some(b), false));
}
