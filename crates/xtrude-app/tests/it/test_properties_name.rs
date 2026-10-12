// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The properties panel names one group or component — shown for exactly one, one undo
//! step, an empty name is not taken.

use crate::common;

use common::{v3, window};
use xtrude_app::tray::properties::Properties;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

fn group(w: &mut MainWindow, name: &str, x: f64) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ],
        &[],
    );
    let g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name));
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    id
}

fn name_of(w: &MainWindow, g: GroupId) -> String {
    w.viewport.scene.doc.find_group(g).unwrap().name.clone()
}

#[test]
fn test_the_field_shows_only_for_one_group() {
    let mut w = window();
    let mut p = Properties::default();
    let (a, b) = (group(&mut w, "Chamber", 0.), group(&mut w, "Flange", 3.));
    w.viewport.scene.selection.insert(EntityRef::Group(a));
    p.refresh(&w);
    assert_eq!(p.named, Some(a));
    assert_eq!(p.name, "Chamber");
    w.viewport.scene.selection.insert(EntityRef::Group(b)); // two: no name
    p.refresh(&w);
    assert!(p.named.is_none());
    w.viewport.scene.selection.clear();
    w.viewport.scene.selection.insert(EntityRef::Group(b)); // another one
    p.refresh(&w);
    assert_eq!(p.name, "Flange");
}

#[test]
fn test_renaming_is_one_undo_step() {
    let mut w = window();
    let mut p = Properties::default();
    let g = group(&mut w, "Group", 0.);
    w.viewport.scene.selection.insert(EntityRef::Group(g));
    p.refresh(&w);
    p.name_edited(&mut w, "Vacuum chamber");
    assert_eq!(name_of(&w, g), "Vacuum chamber");
    assert!(w.viewport.undo());
    assert_eq!(name_of(&w, g), "Group");
}

#[test]
fn test_an_empty_name_is_not_taken() {
    let mut w = window();
    let mut p = Properties::default();
    let g = group(&mut w, "Tapa", 0.);
    w.viewport.scene.selection.insert(EntityRef::Group(g));
    p.refresh(&w);
    p.name_edited(&mut w, "   ");
    assert_eq!(name_of(&w, g), "Tapa");
    assert_eq!(p.name, "Tapa");
}
