// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Make Group over a selection that is not plain loose geometry — it nests
//! groups, answers when it refuses, and never wraps a lone group in itself.

use crate::common;

use common::{context_texts, v3, window};
use glam::DMat4;
use xtrude_app::window::MainWindow;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::SelectMode;

fn quad(m: &mut Mesh, x0: f64) -> FaceId {
    m.add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 2., 0., 0.),
            v3(x0 + 2., 2., 0.),
            v3(x0, 2., 0.),
        ],
        &[],
    )
}

fn group(w: &mut MainWindow, name: &str, x0: f64) -> GroupId {
    let mut m = Mesh::new();
    quad(&mut m, x0);
    let id = add_mesh(&mut w.viewport.scene.doc.meshes, m);
    let g = Group::new(id, Some(name));
    let gid = g.id;
    w.viewport.scene.doc.groups.push(g);
    gid
}

fn ids(w: &MainWindow) -> Vec<GroupId> {
    w.viewport.scene.doc.groups.iter().map(|g| g.id).collect()
}

fn new_groups(w: &MainWindow, before: &[GroupId]) -> Vec<GroupId> {
    ids(w).into_iter().filter(|g| !before.contains(g)).collect()
}

fn select(w: &mut MainWindow, ents: Vec<EntityRef>) {
    w.viewport.scene.select(ents, SelectMode::Replace);
}

fn kids(w: &MainWindow, g: GroupId) -> Vec<GroupId> {
    w.viewport
        .scene
        .doc
        .find_group(g)
        .unwrap()
        .children
        .iter()
        .map(|c| c.id)
        .collect()
}

fn faces(w: &MainWindow, g: GroupId) -> usize {
    let grp = w.viewport.scene.doc.find_group(g).unwrap();
    w.viewport.scene.doc.mesh(grp.mesh).faces.len()
}

#[test]
fn test_empty_selection_says_so_instead_of_going_silent() {
    let mut w = window();
    select(&mut w, vec![]);
    w.status_message = None;
    let before = ids(&w).len();
    w.trigger("make_group");
    assert_eq!(ids(&w).len(), before);
    assert!(w.status_message.is_some(), "the silent no-op is the bug");
}

#[test]
fn test_plain_loose_geometry_still_groups() {
    let mut w = window();
    let f = quad(w.viewport.scene.mesh_mut(), 0.);
    select(&mut w, vec![EntityRef::Face(MeshId::LOOSE, f)]);
    let before = ids(&w).len();
    w.trigger("make_group");
    assert_eq!(ids(&w).len(), before + 1);
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_grouping_a_group_NESTS_it() {
    let mut w = window();
    let (g1, g2) = (group(&mut w, "Spacer", 10.), group(&mut w, "Bracket", 14.));
    select(&mut w, vec![EntityRef::Group(g1), EntityRef::Group(g2)]);
    let before = ids(&w);
    w.trigger("make_group");
    assert!(!ids(&w).contains(&g1) && !ids(&w).contains(&g2));
    let new = new_groups(&w, &before);
    assert_eq!(new.len(), 1);
    assert_eq!(
        kids(&w, new[0]),
        vec![g1, g2],
        "the groups are CHILDREN, not fused"
    );
    assert!(faces(&w, g1) == 1 && faces(&w, g2) == 1);
    assert!(w
        .viewport
        .scene
        .doc
        .find_group(new[0])
        .unwrap()
        .xform
        .is_some());
}

#[test]
fn test_the_loose_part_of_the_selection_goes_to_the_containers_mesh() {
    let mut w = window();
    let f = quad(w.viewport.scene.mesh_mut(), 0.);
    let g = group(&mut w, "Spacer", 10.);
    select(
        &mut w,
        vec![EntityRef::Face(MeshId::LOOSE, f), EntityRef::Group(g)],
    );
    let before = ids(&w);
    let loose = w.viewport.scene.mesh().faces.len();
    w.trigger("make_group");
    let new = new_groups(&w, &before);
    assert_eq!(new.len(), 1);
    assert_eq!(kids(&w, new[0]), vec![g]);
    assert_eq!(faces(&w, new[0]), 1, "the loose face is the parent's mesh");
    assert_eq!(w.viewport.scene.mesh().faces.len(), loose - 1);
    assert_eq!(faces(&w, g), 1, "the bench stays whole");
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_nesting_undoes_in_ONE_step() {
    let mut w = window();
    let f = quad(w.viewport.scene.mesh_mut(), 0.);
    let g = group(&mut w, "Spacer", 10.);
    select(
        &mut w,
        vec![EntityRef::Face(MeshId::LOOSE, f), EntityRef::Group(g)],
    );
    let before = ids(&w);
    let loose = w.viewport.scene.mesh().faces.len();
    w.trigger("make_group");
    assert!(w.viewport.undo());
    assert_eq!(ids(&w), before);
    assert_eq!(w.viewport.scene.mesh().faces.len(), loose);
    assert!(kids(&w, g).is_empty(), "and the bench is not left adopted");
}

#[test]
fn test_a_group_with_children_nests_without_losing_them() {
    let mut w = window();
    let mut deep = Mesh::new();
    quad(&mut deep, 0.);
    let did = add_mesh(&mut w.viewport.scene.doc.meshes, deep);
    let mut child = Group::new(did, Some("Board"));
    child.xform = Some(DMat4::IDENTITY);
    let hid = child.id;
    let parent = group(&mut w, "Spacer", 20.);
    w.viewport
        .scene
        .doc
        .find_group_mut(parent)
        .unwrap()
        .adopt(vec![child]);
    let other = group(&mut w, "Bracket", 40.);
    select(
        &mut w,
        vec![EntityRef::Group(parent), EntityRef::Group(other)],
    );
    let before = ids(&w);
    w.trigger("make_group");
    let new = new_groups(&w, &before);
    assert_eq!(new.len(), 1);
    assert_eq!(kids(&w, new[0]), vec![parent, other]);
    assert_eq!(kids(&w, parent), vec![hid], "the grandchild is still there");
    assert_eq!(faces(&w, hid), 1);
}

#[test]
fn test_the_context_menu_offers_make_group_with_only_groups() {
    let mut w = window();
    let gs: Vec<EntityRef> = (0..4)
        .map(|i| EntityRef::Group(group(&mut w, &format!("G{i}"), 10. + 4. * i as f64)))
        .collect();
    select(&mut w, gs);
    let texts = context_texts(&w);
    assert!(texts.contains(&"Make Group".to_string()));
    assert!(texts.contains(&"Merge Groups".to_string())); // the old one stays
}

#[test]
fn test_a_lone_group_is_not_grouped_with_itself() {
    let mut w = window();
    let g = group(&mut w, "Spacer", 10.);
    select(&mut w, vec![EntityRef::Group(g)]);
    w.status_message = None;
    let before = ids(&w);
    w.trigger("make_group");
    assert!(ids(&w) == before && kids(&w, g).is_empty());
    assert!(w.status_message.is_some());
}

#[test]
fn test_the_menu_does_not_offer_to_group_a_lone_group() {
    let mut w = window();
    let (g1, g2) = (group(&mut w, "Spacer", 10.), group(&mut w, "Bracket", 14.));
    let entries = |w: &mut MainWindow, sel: Vec<EntityRef>| {
        select(w, sel);
        context_texts(w)
    };
    assert!(!entries(&mut w, vec![EntityRef::Group(g1)]).contains(&"Make Group".to_string()));
    assert!(
        entries(&mut w, vec![EntityRef::Group(g1), EntityRef::Group(g2)])
            .contains(&"Make Group".to_string())
    );
    let f = quad(w.viewport.scene.mesh_mut(), 0.);
    assert!(entries(
        &mut w,
        vec![EntityRef::Group(g1), EntityRef::Face(MeshId::LOOSE, f)]
    )
    .contains(&"Make Group".to_string()));
}
