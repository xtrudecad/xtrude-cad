// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The name Make Component suggests: "Component #n", n one more than the
//! components at every depth, skipping a name a group already has.

use crate::common;

use common::{v3, window};
use glam::DMat4;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

fn win() -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    (w, dialogs)
}

fn square(x: f64) -> Vec<glam::DVec3> {
    vec![
        v3(x, 0., 0.),
        v3(x + 1., 0., 0.),
        v3(x + 1., 1., 0.),
        v3(x, 1., 0.),
    ]
}

/// A group named `name` on a mesh of its own, a component when `component`.
fn named(w: &mut MainWindow, name: &str, component: bool) -> Group {
    let mut m = Mesh::new();
    m.add_face(&square(5.), &[]);
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name));
    g.component = component;
    if component {
        g.xform = Some(DMat4::IDENTITY);
    }
    g
}

fn add_top(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

/// Draw a square in the open context, select it, and ask Make Component
/// (cancelled): the name it suggested.
fn suggested(w: &mut MainWindow, dialogs: &Rc<RefCell<ScriptedDialogs>>) -> String {
    assert!(w.viewport.run(AddFaceCommand::new(square(0.))));
    let scene = &mut w.viewport.scene;
    let mid = scene.active_mesh_id();
    scene.selection.clear();
    for f in scene.mesh().face_ids() {
        scene.selection.insert(EntityRef::Face(mid, f));
    }
    w.trigger("make_component");
    let asked = dialogs.borrow().asked.clone();
    let (kind, text) = asked.last().expect("Make Component asks a name");
    assert_eq!(kind, "text");
    text.rsplit('|').next().unwrap().to_string()
}

#[test]
fn test_a_model_of_top_level_components_suggests_as_before() {
    let (mut w, dialogs) = win();
    for name in ["Peg", "Peg"] {
        let g = named(&mut w, name, true);
        add_top(&mut w, g);
    }
    let plate = named(&mut w, "Plate", false);
    add_top(&mut w, plate);
    assert_eq!(suggested(&mut w, &dialogs), "Component #3");
}

#[test]
fn test_nested_components_count() {
    let (mut w, dialogs) = win();
    let bolt = named(&mut w, "Bolt", true);
    let mut leg = named(&mut w, "Leg", true);
    leg.children = vec![bolt];
    add_top(&mut w, leg);
    assert_eq!(suggested(&mut w, &dialogs), "Component #3");
}

#[test]
fn test_a_name_in_use_is_skipped() {
    let (mut w, dialogs) = win();
    let peg = named(&mut w, "Peg", true);
    add_top(&mut w, peg);
    let taken = named(&mut w, "Component #2", false);
    add_top(&mut w, taken);
    assert_eq!(suggested(&mut w, &dialogs), "Component #3");
}

#[test]
fn test_inside_an_open_group_a_nested_name_is_not_repeated() {
    let (mut w, dialogs) = win();
    let first = named(&mut w, "Component #1", true);
    let mut outer = named(&mut w, "Outer", false);
    outer.xform = Some(DMat4::IDENTITY);
    outer.children = vec![first];
    let outer = add_top(&mut w, outer);
    w.viewport.begin_group_edit(outer);
    assert_eq!(suggested(&mut w, &dialogs), "Component #2");
}

#[test]
fn test_an_open_copy_without_children_still_counts() {
    let (mut w, dialogs) = win();
    let first = named(&mut w, "Peg", true);
    let mut src = first.clone();
    let first = add_top(&mut w, first);
    let mut second =
        xtrude_model::group::copy_group(&mut w.viewport.scene.doc.meshes, &mut src, None, false);
    second.xform = Some(DMat4::from_translation(v3(0., 5., 0.)));
    add_top(&mut w, second);
    w.viewport.begin_group_edit(first);
    assert_eq!(suggested(&mut w, &dialogs), "Component #3");
}
