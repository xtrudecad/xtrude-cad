// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Make Group and Make Component inside an open group: the new group or
//! component is a child of the open group, with the geometry where it was;
//! inside a component copy, every copy gets it on leaving, as one undo
//! step.

use crate::common;

use common::{v3, window};
use glam::DMat4;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, world_mesh, Group};
use xtrude_model::ids::GroupId;

fn quad(x: f64, z: f64) -> [glam::DVec3; 4] {
    [
        v3(x, 0., z),
        v3(x + 1., 0., z),
        v3(x + 1., 1., z),
        v3(x, 1., z),
    ]
}

/// A mesh of unit squares at `xs`.
fn squares(xs: &[f64]) -> Mesh {
    let mut m = Mesh::new();
    for &x in xs {
        m.add_face(&quad(x, 0.), &[]);
    }
    m
}

fn win() -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    (w, dialogs)
}

/// A plain group (not a component) holding `mesh`.
fn plain(mesh: xtrude_model::ids::MeshId, name: &str) -> Group {
    let mut g = Group::new(mesh, Some(name));
    g.component = false;
    g
}

/// Push `g` at the top level; its id.
fn add_top(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

fn group(w: &MainWindow, id: GroupId) -> &Group {
    w.viewport.scene.doc.find_group(id).unwrap()
}

/// Select the open group's faces lying at `x` < 3.5, and the groups given.
fn select(w: &mut MainWindow, groups: &[GroupId]) {
    let scene = &mut w.viewport.scene;
    scene.selection.clear();
    let mid = scene.active_mesh_id();
    let m = scene.doc.mesh(mid);
    let faces: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.x < 3.5))
        .collect();
    for f in faces {
        scene.selection.insert(EntityRef::Face(mid, f));
    }
    for &g in groups {
        scene.selection.insert(EntityRef::Group(g));
    }
}

/// The world positions of a group's geometry, rounded and sorted.
fn world_points(w: &MainWindow, id: GroupId) -> Vec<[i64; 3]> {
    let m = world_mesh(&w.viewport.scene.doc.meshes, group(w, id));
    let mut v: Vec<[i64; 3]> = m
        .verts
        .iter()
        .map(|(_, v)| v.position.to_array().map(|c| (c * 1e6).round() as i64))
        .collect();
    v.sort();
    v
}

#[test]
fn test_make_group_in_an_open_group_nests_it() {
    let (mut w, _dialogs) = win();
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0., 2., 4.]));
    let outer = add_top(&mut w, plain(mesh, "Outer"));
    let before = world_points(&w, outer);
    w.viewport.begin_group_edit(outer);
    select(&mut w, &[]);
    w.trigger("make_group");
    assert_eq!(
        w.viewport.scene.doc.groups.len(),
        1,
        "nothing at the top level"
    );
    let o = group(&w, outer);
    assert_eq!(o.children.len(), 1, "the new group is in the open one");
    let inner = o.children[0].id;
    assert_eq!(w.viewport.scene.mesh().faces.len(), 1);
    assert_eq!(world_points(&w, inner).len(), 8, "two squares");
    assert!(w
        .viewport
        .scene
        .selection
        .contains(&EntityRef::Group(inner)));
    assert_eq!(
        world_points(&w, outer),
        before,
        "the geometry is where it was"
    );
    assert!(w.viewport.undo());
    assert!(group(&w, outer).children.is_empty());
    assert_eq!(w.viewport.scene.mesh().faces.len(), 3);
}

#[test]
fn test_make_group_over_a_child_and_faces_nests_both() {
    let (mut w, _dialogs) = win();
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0., 2., 4.]));
    let mut bolt_mesh = Mesh::new();
    bolt_mesh.add_face(&quad(0., 2.), &[]);
    let bolt = Group::new(
        add_mesh(&mut w.viewport.scene.doc.meshes, bolt_mesh),
        Some("Bolt"),
    );
    let bolt_id = bolt.id;
    let mut outer = plain(mesh, "Outer");
    outer.adopt(vec![bolt]);
    let outer = add_top(&mut w, outer);
    w.viewport.begin_group_edit(outer);
    select(&mut w, &[bolt_id]);
    w.trigger("make_group");
    assert_eq!(w.viewport.scene.doc.groups.len(), 1);
    let o = group(&w, outer);
    assert_eq!(o.children.len(), 1);
    let container = &o.children[0];
    assert_ne!(container.id, bolt_id);
    assert_eq!(container.children.len(), 1);
    assert_eq!(container.children[0].id, bolt_id);
    assert_eq!(w.viewport.scene.doc.mesh(container.mesh).faces.len(), 2);
}

#[test]
fn test_make_component_in_an_open_group_nests_it() {
    let (mut w, dialogs) = win();
    dialogs.borrow_mut().texts.push(Some("Cap".into()));
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0., 2., 4.]));
    let outer = add_top(&mut w, plain(mesh, "Outer"));
    let before = world_points(&w, outer);
    w.viewport.begin_group_edit(outer);
    select(&mut w, &[]);
    w.trigger("make_component");
    assert_eq!(
        w.viewport.scene.doc.groups.len(),
        1,
        "nothing at the top level"
    );
    let o = group(&w, outer);
    assert_eq!(o.children.len(), 1);
    let cap = &o.children[0];
    assert!(cap.is_component() && cap.name == "Cap");
    w.viewport.end_group_edit();
    assert_eq!(
        world_points(&w, outer),
        before,
        "the geometry is where it was"
    );
}

#[test]
fn test_make_component_of_a_child_group_converts_it_in_place() {
    let (mut w, dialogs) = win();
    dialogs.borrow_mut().texts.push(Some("Bolt".into()));
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[4.]));
    let bolt_mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0.]));
    let bolt = plain(bolt_mesh, "Part");
    let bolt_id = bolt.id;
    let mut outer = plain(mesh, "Outer");
    outer.adopt(vec![bolt]);
    let outer = add_top(&mut w, outer);
    w.viewport.begin_group_edit(outer);
    w.viewport.scene.selection.clear();
    w.viewport.scene.selection.insert(EntityRef::Group(bolt_id));
    w.trigger("make_component");
    let b = group(&w, bolt_id);
    assert!(b.is_component() && b.name == "Bolt");
    assert_eq!(group(&w, outer).children.len(), 1);
    assert_eq!(w.viewport.scene.doc.groups.len(), 1);
}

/// A component of three squares placed twice, the second turned a quarter
/// round about Z at x = 10.
fn two_copies(w: &mut MainWindow) -> [GroupId; 2] {
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0., 2., 4.]));
    let mut first = Group::new(mesh, Some("Plate"));
    first.xform = Some(DMat4::IDENTITY);
    first.component = true;
    let mut src = first.clone();
    let first = add_top(w, first);
    let mut second = copy_group(&mut w.viewport.scene.doc.meshes, &mut src, None, false);
    second.xform =
        Some(DMat4::from_translation(v3(10., 0., 0.)) * DMat4::from_rotation_z(90f64.to_radians()));
    [first, add_top(w, second)]
}

#[test]
fn test_make_component_in_a_copy_reaches_the_other_copy_in_one_undo() {
    let (mut w, dialogs) = win();
    dialogs.borrow_mut().texts.push(Some("Cap".into()));
    let copies = two_copies(&mut w);
    let before = world_points(&w, copies[0]);
    w.viewport.begin_group_edit(copies[0]);
    select(&mut w, &[]);
    w.trigger("make_component");
    w.viewport.end_group_edit();
    assert_eq!(
        w.viewport.scene.doc.groups.len(),
        2,
        "nothing at the top level"
    );
    let caps: Vec<&Group> = copies
        .iter()
        .map(|&c| {
            let kids = &group(&w, c).children;
            assert_eq!(kids.len(), 1, "each copy holds the new component");
            &kids[0]
        })
        .collect();
    assert_ne!(caps[0].id, caps[1].id, "each copy its own GroupId");
    assert!(caps.iter().all(|c| c.is_component() && c.name == "Cap"));
    assert_eq!(caps[0].mesh, caps[1].mesh, "one definition");
    assert_eq!(caps[0].xform, caps[1].xform, "at the same local place");
    assert_eq!(
        world_points(&w, copies[0]),
        before,
        "the geometry is where it was"
    );
    assert_eq!(w.viewport.history.undo_len(), 1, "one undo step");
    assert!(w.viewport.undo());
    assert!(copies.iter().all(|&c| group(&w, c).children.is_empty()));
}

#[test]
fn test_make_component_of_a_part_in_a_copy_converts_it_in_every_copy() {
    let (mut w, dialogs) = win();
    dialogs.borrow_mut().texts.push(Some("Bolt".into()));
    let own = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[4.]));
    let part_mesh = add_mesh(&mut w.viewport.scene.doc.meshes, squares(&[0.]));
    // A part of the definition: placed, not a component of its own.
    let mut part = plain(part_mesh, "Part");
    part.xform = Some(DMat4::IDENTITY);
    let mut leg = Group::new(own, Some("Leg"));
    leg.xform = Some(DMat4::IDENTITY);
    leg.children = vec![part];
    let mut src = leg.clone();
    let first = add_top(&mut w, leg);
    let mut second = copy_group(&mut w.viewport.scene.doc.meshes, &mut src, None, false);
    second.xform = Some(DMat4::from_translation(v3(0., 5., 0.)));
    let second = add_top(&mut w, second);
    w.viewport.begin_group_edit(second);
    let part = group(&w, second).children[0].id;
    w.viewport.scene.selection.clear();
    w.viewport.scene.selection.insert(EntityRef::Group(part));
    w.trigger("make_component");
    w.viewport.end_group_edit();
    let parts: Vec<&Group> = [first, second]
        .iter()
        .map(|&c| &group(&w, c).children[0])
        .collect();
    assert!(
        parts.iter().all(|p| p.is_component() && p.name == "Bolt"),
        "converted in every copy"
    );
    assert_eq!(parts[0].mesh, parts[1].mesh, "one definition");
    assert_eq!(parts[0].xform, parts[1].xform, "at the same local place");
    assert_eq!(w.viewport.history.undo_len(), 1, "one undo step");
    assert!(w.viewport.undo());
    assert!([first, second]
        .iter()
        .all(|&c| !group(&w, c).children[0].is_component()));
}
