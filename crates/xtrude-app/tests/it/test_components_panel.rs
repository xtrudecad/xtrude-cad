// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Components panel: a row per definition with its copies counted at
//! every level (inside groups, inside other components' definitions), and a
//! click that opens the first copy's parents and selects the copies there.

use crate::common;

use common::{v3, window};
use glam::{DMat4, DVec3};
use xtrude_app::tray::components::Components;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::commands::{AddFaceCommand, MakeGroupCommand, MakeNestedGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn quad(w: &mut MainWindow, x: f64) -> MeshId {
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
    add_mesh(&mut w.viewport.scene.doc.meshes, m)
}

fn add_group(w: &mut MainWindow, x: f64, name: &str) -> GroupId {
    let g = Group::new(quad(w, x), Some(name));
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    id
}

/// A copy of the component `mesh`, placed at `x` on the top level.
fn add_copy(w: &mut MainWindow, mesh: MeshId, x: f64, name: &str) -> GroupId {
    let mut g = Group::new(mesh, Some(name));
    g.xform = Some(DMat4::from_translation(DVec3::new(x, 0., 0.)));
    g.component = true;
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    id
}

/// Nest `groups` into a new top-level group `name`; its id.
fn nest(w: &mut MainWindow, groups: Vec<GroupId>, name: &str) -> GroupId {
    let mut c = MakeNestedGroupCommand::new(w.viewport.scene.mesh(), &[], &[], groups, Some(name));
    w.viewport.execute(&mut c);
    w.viewport.scene.doc.groups.last().unwrap().id
}

/// `Assembly ▸ (Housing ▸ (Lid, Base), Peg)`, Peg a component.
struct Assembly {
    w: MainWindow,
    assembly: GroupId,
    peg: GroupId,
    peg_mesh: MeshId,
}

fn assembly() -> Assembly {
    let mut w = window();
    let lid = add_group(&mut w, 0., "Lid");
    let base = add_group(&mut w, 2., "Base");
    let housing = nest(&mut w, vec![lid, base], "Housing");
    let peg_mesh = quad(&mut w, 0.);
    let peg = add_copy(&mut w, peg_mesh, 4., "Peg");
    let assembly = nest(&mut w, vec![housing, peg], "Assembly");
    Assembly {
        w,
        assembly,
        peg,
        peg_mesh,
    }
}

fn selection(w: &MainWindow) -> Vec<EntityRef> {
    let mut sel: Vec<_> = w.viewport.scene.selection.iter().copied().collect();
    sel.sort();
    sel
}

#[test]
fn test_a_component_only_inside_a_group_gets_its_row() {
    let a = assembly();
    let mut panel = Components::default();
    panel.refresh_in_model(&a.w);
    assert_eq!(panel.rows.len(), 1); // not "No components yet"
    assert_eq!(panel.rows[0].label, "Peg  (1×, 1 faces)");
    assert_eq!(panel.rows[0].groups, vec![a.peg]);
}

#[test]
fn test_copies_are_counted_across_levels() {
    let mut a = assembly();
    let top = add_copy(&mut a.w, a.peg_mesh, 8., "Peg");
    let mut panel = Components::default();
    panel.refresh_in_model(&a.w);
    assert_eq!(panel.rows.len(), 1);
    assert_eq!(panel.rows[0].label, "Peg  (2×, 1 faces)");
    let mut groups = panel.rows[0].groups.clone();
    groups.sort();
    let mut want = vec![a.peg, top];
    want.sort();
    assert_eq!(groups, want);
}

#[test]
fn test_a_component_in_a_components_definition_counts_each_copy() {
    // Box holds a Peg in its definition; a copy of Box holds its own Peg,
    // a group of its own (same mesh), selectable inside that copy.
    let mut w = window();
    let peg_mesh = quad(&mut w, 0.);
    let box_mesh = quad(&mut w, 5.);
    let mut peg = Group::new(peg_mesh, Some("Peg"));
    peg.xform = Some(DMat4::IDENTITY);
    peg.component = true;
    let mut boxed = Group::new(box_mesh, Some("Box"));
    boxed.xform = Some(DMat4::IDENTITY);
    boxed.component = true;
    boxed.children = vec![peg];
    let copy = copy_group(
        &mut w.viewport.scene.doc.meshes,
        &mut boxed,
        Some(DVec3::new(10., 0., 0.)),
        false,
    );
    let pegs = [boxed.children[0].id, copy.children[0].id];
    assert_ne!(pegs[0], pegs[1]);
    w.viewport.scene.doc.groups.push(boxed);
    w.viewport.scene.doc.groups.push(copy);
    let mut panel = Components::default();
    panel.refresh_in_model(&w);
    let labels: Vec<&str> = panel.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, vec!["Box  (2×, 1 faces)", "Peg  (2×, 1 faces)"]);
}

#[test]
fn test_clicking_a_nested_components_row_opens_its_parent_and_selects_it() {
    let mut a = assembly();
    // A second copy at the top level: another level, left unselected.
    add_copy(&mut a.w, a.peg_mesh, 8., "Peg");
    let mut panel = Components::default();
    panel.refresh_in_model(&a.w);
    let first = panel.rows[0].groups[0];
    assert_eq!(first, a.peg); // the walk meets the nested copy first
    panel.select(&mut a.w, 0);
    assert_eq!(a.w.viewport.scene.edit_group(), Some(a.assembly));
    assert_eq!(selection(&a.w), vec![EntityRef::Group(a.peg)]);
}

#[test]
fn test_clicking_a_top_level_row_selects_every_copy() {
    let mut w = window();
    let mesh = quad(&mut w, 0.);
    let one = add_copy(&mut w, mesh, 0., "Pin");
    let two = add_copy(&mut w, mesh, 3., "Pin");
    let mut panel = Components::default();
    panel.refresh_in_model(&w);
    assert_eq!(panel.rows[0].label, "Pin  (2×, 1 faces)");
    panel.select(&mut w, 0);
    assert!(w.viewport.scene.edit_group().is_none());
    let mut want = vec![EntityRef::Group(one), EntityRef::Group(two)];
    want.sort();
    assert_eq!(selection(&w), want);
}

fn row_labels(w: &MainWindow) -> Vec<String> {
    let mut c = Components::default();
    c.refresh_in_model(w);
    c.rows.into_iter().map(|r| r.label).collect()
}

#[test]
fn test_an_open_copy_still_counts_with_its_definition() {
    let mut w = window();
    let peg_mesh = quad(&mut w, 0.);
    let first = add_copy(&mut w, peg_mesh, 0., "Peg");
    add_copy(&mut w, peg_mesh, 3., "Peg");
    let shown = row_labels(&w);
    assert_eq!(shown, ["Peg  (2×, 1 faces)"]);
    w.viewport.begin_group_edit(first);
    assert_eq!(row_labels(&w), shown, "a copy without children, open");
    w.viewport.end_group_edit();
    assert_eq!(row_labels(&w), shown);
}

#[test]
fn test_an_open_container_copy_still_counts_with_its_definition() {
    let mut w = window();
    let leg_mesh = quad(&mut w, 0.);
    let first = add_copy(&mut w, leg_mesh, 0., "Leg");
    // A part of the definition: placed, not a component of its own.
    let mut foot = Group::new(quad(&mut w, 5.), Some("Foot"));
    foot.xform = Some(DMat4::IDENTITY);
    foot.component = false;
    let leg = w.viewport.scene.doc.find_group_mut(first).unwrap();
    leg.children = vec![foot];
    let mut src = leg.clone();
    let mut second = copy_group(&mut w.viewport.scene.doc.meshes, &mut src, None, false);
    second.xform = Some(DMat4::from_translation(DVec3::new(0., 3., 0.)));
    w.viewport.scene.doc.groups.push(second);
    let shown = row_labels(&w);
    assert_eq!(shown, ["Leg  (2×, 1 faces)"]);
    w.viewport.begin_group_edit(first);
    assert_eq!(row_labels(&w), shown, "a container copy, open");
}

/// Two Pegs of one face, the first one open.
fn open_peg() -> MainWindow {
    let mut w = window();
    let peg_mesh = quad(&mut w, 0.);
    let first = add_copy(&mut w, peg_mesh, 0., "Peg");
    add_copy(&mut w, peg_mesh, 3., "Peg");
    w.viewport.begin_group_edit(first);
    w
}

#[test]
fn test_a_face_drawn_in_an_open_copy_counts_at_once() {
    let mut w = open_peg();
    assert!(w.viewport.run(AddFaceCommand::new(vec![
        v3(0., 2., 0.),
        v3(1., 2., 0.),
        v3(1., 3., 0.),
        v3(0., 3., 0.),
    ])));
    assert_eq!(row_labels(&w), ["Peg  (2×, 2 faces)"], "while open");
    w.viewport.end_group_edit();
    assert_eq!(row_labels(&w), ["Peg  (2×, 2 faces)"], "left");
    assert!(w.viewport.undo());
    assert_eq!(row_labels(&w), ["Peg  (2×, 1 faces)"], "undone");
}

#[test]
fn test_make_group_in_an_open_copy_counts_at_once() {
    let mut w = open_peg();
    let m = w.viewport.scene.mesh();
    let mut mk = MakeGroupCommand::new(m, &m.face_ids(), &[], false, Some("Cap"));
    assert!(w.viewport.execute(&mut mk));
    assert_eq!(row_labels(&w), ["Peg  (2×, 0 faces)"], "while open");
    w.viewport.end_group_edit();
    assert_eq!(row_labels(&w), ["Peg  (2×, 0 faces)"], "left");
    assert!(w.viewport.undo());
    assert_eq!(row_labels(&w), ["Peg  (2×, 1 faces)"], "undone");
}
