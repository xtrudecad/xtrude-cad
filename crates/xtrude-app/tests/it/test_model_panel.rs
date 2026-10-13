// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Model panel's tree, beyond the plain tree (`test_model_tree`): a
//! component row counts its copies and selects them all, «Components only»
//! lists each definition once, the optional columns measure each row
//! along the model axes, each top-level row is a link named as a robot
//! description names it, Split into Pieces runs on a row, and a group
//! picked in the canvas scrolls into view.

use crate::common;

use common::{v3, window};
use xtrude_app::tray::model_tree::{shown_columns, ModelTree, Row};
use xtrude_app::window::MainWindow;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn boxm(m: &mut Mesh, s: (f64, f64, f64), at: (f64, f64, f64), color: Option<[f64; 3]>) {
    let mut c = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                c.push(v3(at.0 + dx * s.0, at.1 + dy * s.1, at.2 + dz * s.2));
            }
        }
    }
    for q in QUADS {
        let f = m.add_face(&q.map(|k| c[k]), &[]);
        if let Some(col) = color {
            m.face_mut(f).attrs.paint.color = Some(col);
        }
    }
}

/// A part placed by `xform`: a group, not a component.
fn part(w: &mut MainWindow, m: Mesh, name: &str, xform: DMat4) -> Group {
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name));
    g.xform = Some(xform);
    g.component = false;
    g
}

fn holder(w: &mut MainWindow, name: &str, kids: Vec<Group>) -> Group {
    let mut g = Group::new(
        add_mesh(&mut w.viewport.scene.doc.meshes, Mesh::new()),
        Some(name),
    );
    g.adopt(kids);
    g
}

fn add(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

/// A seat and two legs, one splayed, in a «stool» moved 1 m along red.
fn stool(w: &mut MainWindow) -> GroupId {
    let wood = Some([0.6, 0.4, 0.2]);
    let mut seat = Mesh::new();
    boxm(&mut seat, (0.40, 0.40, 0.03), (0., 0., 0.58), wood);
    let mut a = Mesh::new();
    boxm(&mut a, (0.04, 0.04, 0.58), (0., 0., 0.), wood);
    let mut b = Mesh::new();
    boxm(&mut b, (0.04, 0.04, 0.58), (0., 0., 0.), wood);
    let splay = DMat4::from_translation(v3(0.36, 0., 0.)) * rotation_deg(DVec3::Y, 10.0);
    let kids = vec![
        part(w, seat, "seat", DMat4::IDENTITY),
        part(w, a, "leg A", DMat4::IDENTITY),
        part(w, b, "leg B", splay),
    ];
    let mut g = holder(w, "stool", kids);
    g.xform = Some(DMat4::from_translation(v3(1., 0., 0.)));
    g.component = false;
    add(w, g)
}

/// A one-face component mesh and `n` copies of it on the top level.
fn pins(w: &mut MainWindow, n: usize) -> (MeshId, Vec<GroupId>) {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, m);
    let ids = (0..n)
        .map(|i| {
            let mut g = Group::new(mesh, Some("Pin"));
            g.xform = Some(DMat4::from_translation(v3(i as f64 * 3., 0., 0.)));
            g.component = true;
            add(w, g)
        })
        .collect();
    (mesh, ids)
}

fn tree(w: &MainWindow) -> ModelTree {
    let mut t = ModelTree::default();
    t.refresh(w);
    t
}

fn row(t: &ModelTree, g: GroupId) -> Row {
    t.row(g).cloned().expect("a row")
}

fn selection(w: &MainWindow) -> Vec<EntityRef> {
    let mut sel: Vec<_> = w.viewport.scene.selection.iter().copied().collect();
    sel.sort();
    sel
}

#[test]
fn test_a_component_row_counts_its_copies_and_a_group_row_one() {
    let mut w = window();
    let (_, ids) = pins(&mut w, 3);
    let s = stool(&mut w);
    let t = tree(&w);
    assert_eq!(row(&t, ids[0]).copies, 3);
    assert!(row(&t, ids[0]).component);
    assert_eq!(row(&t, s).copies, 1);
}

#[test]
fn test_select_all_copies_selects_the_copies_at_the_rows_level() {
    let mut w = window();
    let (mesh, ids) = pins(&mut w, 2);
    // a third copy inside a group: another level, left unselected
    let mut nested = Group::new(mesh, Some("Pin"));
    nested.xform = Some(DMat4::IDENTITY);
    nested.component = true;
    let inner = nested.id;
    let g = holder(&mut w, "box", vec![nested]);
    let boxed = add(&mut w, g);
    let mut t = tree(&w);
    assert_eq!(row(&t, ids[1]).copies, 3);
    t.select_copies(&mut w, ids[1]);
    assert!(w.viewport.scene.edit_group().is_none());
    let mut want = vec![EntityRef::Group(ids[0]), EntityRef::Group(ids[1])];
    want.sort();
    assert_eq!(selection(&w), want);
    t.select_copies(&mut w, inner);
    assert_eq!(w.viewport.scene.edit_group(), Some(boxed));
    assert_eq!(selection(&w), [EntityRef::Group(inner)]);
}

#[test]
fn test_components_only_lists_each_definition_once_and_a_click_selects_its_copies() {
    let mut w = window();
    let (_, ids) = pins(&mut w, 2);
    stool(&mut w);
    let mut t = tree(&w);
    let labels: Vec<&str> = t.components.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Pin  (2×, 1 faces)"]);
    t.select_definition(&mut w, 0);
    let mut want = vec![EntityRef::Group(ids[0]), EntityRef::Group(ids[1])];
    want.sort();
    assert_eq!(selection(&w), want);
}

#[test]
fn test_the_columns_are_off_by_default_and_measure_along_the_model_axes() {
    let mut w = window();
    let s = stool(&mut w);
    let mut t = tree(&w);
    assert!(shown_columns().is_empty());
    assert_eq!(row(&t, s).size, "");
    for key in ["size", "faces", "color"] {
        t.set_column_shown(&w, key, true);
    }
    assert_eq!(shown_columns(), ["size", "faces", "color"]);
    let stool_row = row(&t, s);
    let seat = stool_row
        .children
        .iter()
        .find(|r| r.name == "seat")
        .unwrap();
    // the seat moved with the stool, measured in the document's mm
    assert_eq!(seat.size, "400.00 × 400.00 × 30.00 mm");
    assert_eq!(seat.faces, "6");
    assert_eq!(seat.color, "#996633");
    assert_eq!(stool_row.faces, "18");
    // the splayed leg's box along the model axes, not its own
    let leg = stool_row
        .children
        .iter()
        .find(|r| r.name == "leg B")
        .unwrap();
    assert!(leg.size.starts_with("140.11 × 40.00 × "), "{}", leg.size);
    // remembered, in the columns' order
    t.set_column_shown(&w, "size", false);
    assert_eq!(shown_columns(), ["faces", "color"]);
    let _next = MainWindow::new();
    assert_eq!(shown_columns(), ["faces", "color"]);
}

#[test]
fn test_the_columns_follow_an_edit() {
    let mut w = window();
    let s = stool(&mut w);
    let mut t = tree(&w);
    t.set_column_shown(&w, "faces", true);
    assert_eq!(row(&t, s).faces, "18");
    let mut extra = Mesh::new();
    boxm(&mut extra, (0.1, 0.1, 0.1), (2., 0., 0.), None);
    let rung = part(&mut w, extra, "rung", DMat4::IDENTITY);
    w.viewport
        .scene
        .doc
        .find_group_mut(s)
        .unwrap()
        .children
        .push(rung);
    w.viewport.scene.version += 1;
    t.refresh(&w);
    assert_eq!(row(&t, s).faces, "24");
}

#[test]
fn test_each_top_level_row_is_a_link_and_a_nested_one_merges_into_it() {
    let mut w = window();
    let s = stool(&mut w);
    let t = tree(&w);
    let top = row(&t, s);
    assert!(top.top_level);
    assert_eq!(top.link, "stool");
    assert_eq!(top.link_warning(), None);
    let seat = &top.children[0];
    assert!(!seat.top_level);
    assert_eq!(seat.link, "stool");
    assert_eq!(seat.link_warning(), None); // a nested row is no link
    assert_eq!(seat.link_tip(), "Merged into link «stool»");
}

#[test]
fn test_a_link_named_otherwise_than_its_group_warns() {
    let mut w = window();
    let one = stool(&mut w);
    let two = stool(&mut w);
    w.viewport.scene.doc.find_group_mut(one).unwrap().name = "upper arm".into();
    w.viewport.scene.doc.find_group_mut(two).unwrap().name = "upper_arm".into();
    w.viewport.scene.version += 1;
    let t = tree(&w);
    assert_eq!(row(&t, one).link, "upper_arm");
    assert!(row(&t, one)
        .link_warning()
        .is_some_and(|m| m.starts_with("Exports as «upper_arm»")));
    assert_eq!(row(&t, two).link, "upper_arm_2");
    assert!(row(&t, two).link_warning().is_some());
    // renamed apart: no warning left
    w.viewport.scene.doc.find_group_mut(one).unwrap().name = "base".into();
    w.viewport.scene.version += 1;
    let t = tree(&w);
    assert_eq!(row(&t, one).link_warning(), None);
    assert_eq!(row(&t, two).link, "upper_arm");
    assert_eq!(row(&t, two).link_warning(), None);
}

#[test]
fn test_split_into_pieces_runs_on_a_row() {
    let mut w = window();
    let mut m = Mesh::new();
    boxm(&mut m, (0.1, 0.1, 0.1), (0., 0., 0.), None);
    boxm(&mut m, (0.1, 0.1, 0.1), (1., 0., 0.), None);
    let g = Group::new(
        add_mesh(&mut w.viewport.scene.doc.meshes, m),
        Some("two blocks"),
    );
    let blocks = add(&mut w, g);
    let mut t = tree(&w);
    t.split(&mut w, blocks);
    let names: Vec<String> = row(&t, blocks)
        .children
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(names, ["Piece 1", "Piece 2"]);
    assert!(t.expanded.contains(&blocks));
    w.viewport.undo();
    t.refresh(&w);
    assert!(row(&t, blocks).children.is_empty());
}

#[test]
fn test_a_group_picked_in_the_canvas_scrolls_into_view_and_a_tree_click_does_not() {
    let mut w = window();
    let s = stool(&mut w);
    let mut t = tree(&w);
    assert_eq!(t.scroll_to, None);
    let leg = w.viewport.scene.doc.find_group(s).unwrap().children[2].id;
    w.viewport.scene.begin_group_edit(s);
    w.viewport.scene.selection.insert(EntityRef::Group(leg));
    w.viewport.scene.version += 1;
    t.refresh(&w);
    assert_eq!(t.scroll_to, Some(leg));
    assert!(t.expanded.contains(&s));
    t.scroll_to = None; // taken by the shell
    let seat = w.viewport.scene.doc.find_group(s).unwrap().children[0].id;
    t.tree_selection(&mut w, &[seat], Some(seat), false);
    assert_eq!(selection(&w), [EntityRef::Group(seat)]);
    assert_eq!(t.scroll_to, None);
}

#[test]
fn test_the_eye_hides_and_shows_a_row_in_one_undo_step() {
    let mut w = window();
    let s = stool(&mut w);
    let mut t = tree(&w);
    t.set_hidden(&mut w, &[s], true);
    assert!(row(&t, s).hidden);
    w.viewport.undo();
    t.refresh(&w);
    assert!(!row(&t, s).hidden);
}
