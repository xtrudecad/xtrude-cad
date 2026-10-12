// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Grouping a group together with loose geometry NESTS it (no question asked);
//! grouping loose geometry alone still works; Make Component over both holds
//! both.

use crate::common;

use common::{v3, window};
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_app::window::MainWindow;
use xtrude_model::commands::{AddFaceCommand, MakeGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::{GroupId, MeshId};

fn quad(x: f64) -> Vec<glam::DVec3> {
    vec![
        v3(x, 0., 0.),
        v3(x + 1., 0., 0.),
        v3(x + 1., 1., 0.),
        v3(x, 1., 0.),
    ]
}

fn win_with_a_group_and_loose_geometry() -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    let vp = &mut w.viewport;
    vp.execute(&mut AddFaceCommand::new(quad(0.)));
    let m = vp.scene.mesh();
    let mut mk = MakeGroupCommand::new(m, &m.face_ids(), &m.edge_ids(), false, None);
    vp.execute(&mut mk);
    vp.execute(&mut AddFaceCommand::new(quad(3.)));
    (w, dialogs)
}

fn group_ids(w: &MainWindow) -> Vec<GroupId> {
    w.viewport.scene.doc.groups.iter().map(|g| g.id).collect()
}

fn select_all_top(w: &mut MainWindow, with_groups: bool) {
    let vp = &mut w.viewport;
    vp.scene.clear_selection();
    let faces = vp.scene.mesh().face_ids();
    for f in faces {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    if with_groups {
        for g in group_ids(w) {
            w.viewport.scene.selection.insert(EntityRef::Group(g));
        }
    }
}

fn child_ids(w: &MainWindow, g: GroupId) -> Vec<GroupId> {
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

#[test]
fn test_grouping_a_group_with_loose_geometry_nests_it() {
    let (mut w, dialogs) = win_with_a_group_and_loose_geometry();
    let before = group_ids(&w);
    let loose = w.viewport.scene.mesh().face_ids().len();
    select_all_top(&mut w, true);
    w.trigger("make_group");
    assert!(dialogs.borrow().asked.is_empty(), "nothing to ask any more");
    let new: Vec<GroupId> = group_ids(&w)
        .into_iter()
        .filter(|g| !before.contains(g))
        .collect();
    assert_eq!(new.len(), 1);
    let parent = new[0];
    assert_eq!(child_ids(&w, parent), before, "the old group is its child");
    let pm = w.viewport.scene.doc.find_group(parent).unwrap().mesh;
    assert_eq!(w.viewport.scene.doc.mesh(pm).face_ids().len(), 1); // the loose face
    assert_eq!(w.viewport.scene.mesh().face_ids().len(), loose - 1);
}

#[test]
fn test_grouping_only_loose_geometry_still_works() {
    let (mut w, _dialogs) = win_with_a_group_and_loose_geometry();
    let before = group_ids(&w).len();
    select_all_top(&mut w, false);
    w.trigger("make_group");
    assert_eq!(group_ids(&w).len(), before + 1);
}

#[test]
fn test_component_from_loose_geometry_plus_a_group_holds_both() {
    let (mut w, dialogs) = win_with_a_group_and_loose_geometry();
    dialogs.borrow_mut().texts.push(Some("Spacer".into()));
    let inner = group_ids(&w);
    let before = inner.len();
    select_all_top(&mut w, true);
    w.trigger("make_component");
    assert_eq!(group_ids(&w).len(), before - inner.len() + 1);
    let comp = group_ids(&w)
        .into_iter()
        .find(|g| !inner.contains(g))
        .unwrap();
    let c = w.viewport.scene.doc.find_group(comp).unwrap();
    assert!(c.is_component() && c.name == "Spacer");
    assert_eq!(child_ids(&w, comp), inner);
    assert!(w.viewport.scene.mesh().face_ids().is_empty());
}
