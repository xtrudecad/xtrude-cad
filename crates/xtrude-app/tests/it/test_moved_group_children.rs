// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Groups nested in a moved plain group stay plain groups when it is
//! opened and left: its matrix goes down into them, not a component's
//! place.

use crate::common;

use common::{v3, window};
use xtrude_app::tray::components::Components;
use xtrude_app::window::MainWindow;
use xtrude_model::commands::{
    AddFaceCommand, MakeGroupCommand, MakeNestedGroupCommand, MoveGroupCommand,
};
use xtrude_model::ids::GroupId;

/// A unit square at `x` made into a group with Make Group; its id.
fn grouped_square(w: &mut MainWindow, x: f64) -> GroupId {
    let vp = &mut w.viewport;
    let mut add = AddFaceCommand::new(vec![
        v3(x, 0., 0.),
        v3(x + 1., 0., 0.),
        v3(x + 1., 1., 0.),
        v3(x, 1., 0.),
    ]);
    assert!(vp.execute(&mut add));
    let face = add.face.unwrap();
    let mut mk = MakeGroupCommand::new(vp.scene.mesh(), &[face], &[], false, None);
    assert!(vp.execute(&mut mk));
    mk.group.unwrap()
}

/// `Box ▸ (two groups)`, made with the commands and moved by 1 along X.
fn moved_box(w: &mut MainWindow) -> (GroupId, [GroupId; 2]) {
    let kids = [grouped_square(w, 0.), grouped_square(w, 2.)];
    let vp = &mut w.viewport;
    let mut nest = MakeNestedGroupCommand::new(vp.scene.mesh(), &[], &[], kids.to_vec(), None);
    assert!(vp.execute(&mut nest));
    let outer = nest.container.unwrap();
    assert!(vp.run(MoveGroupCommand {
        group: outer,
        delta: v3(1., 0., 0.),
    }));
    (outer, kids)
}

fn components(w: &MainWindow) -> Vec<String> {
    let mut c = Components::default();
    c.refresh_in_model(w);
    c.rows.into_iter().map(|r| r.label).collect()
}

fn any_component(w: &MainWindow, ids: &[GroupId]) -> bool {
    ids.iter()
        .any(|&g| w.viewport.scene.doc.find_group(g).unwrap().is_component())
}

#[test]
fn test_groups_in_a_moved_group_stay_groups_while_it_is_open() {
    let mut w = window();
    let (outer, kids) = moved_box(&mut w);
    assert!(components(&w).is_empty());
    w.viewport.begin_group_edit(outer);
    assert!(!any_component(&w, &kids));
    assert!(components(&w).is_empty(), "{:?}", components(&w));
    w.viewport.end_group_edit();
    assert!(!any_component(&w, &kids), "and once it is left");
    assert!(components(&w).is_empty(), "{:?}", components(&w));
}

#[test]
fn test_groups_in_a_moved_group_stay_groups_through_a_save() {
    let mut w = window();
    let (outer, _kids) = moved_box(&mut w);
    w.viewport.begin_group_edit(outer);
    w.viewport.end_group_edit();
    let dir = common::TmpDir::new();
    let path = dir.join("box.igz");
    xtrude_io::igz::save_scene(&w.viewport.scene, &path).expect("save");
    let mut back = common::stub();
    xtrude_io::igz::load_into(&mut back.scene, &path, None).expect("load");
    let reloaded = &back.scene.doc.groups[0];
    assert_eq!(reloaded.children.len(), 2);
    assert!(reloaded.children.iter().all(|c| !c.is_component()));
}
