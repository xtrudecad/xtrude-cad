// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3D Text through the tool and the main window (the text itself lives in
//! crates/xtrude-model).

use crate::common;
use crate::common::FakeProbe;

use std::cell::RefCell;
use std::rc::Rc;

use common::{stub, window};
use glam::DVec3;
use xtrude_app::tools::place_group::PlaceGroupTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::ViewportEvent;
use xtrude_app::window::ScriptedDialogs;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group, MeshTable};
use xtrude_model::scene::SelectMode;
use xtrude_model::text3d::{make_text_group, TextParams};

/// Double-click keeps its usual meaning — it ENTERS the group, whose
/// children are the letters.
#[test]
fn test_select_double_click_on_a_text_enters_it_like_any_group() {
    let mut vp = stub();
    let text = make_text_group(&mut vp.scene.doc.meshes, &TextParams::new("Ab", "Sans")).unwrap();
    let id = text.id;
    vp.scene.doc.groups.push(text);
    FakeProbe::of(&mut vp).pick_group(move |_, _| Some(id));
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    SelectTool::new().on_double_click(&mut vp, &ToolContext::at(DVec3::ZERO));
    assert_eq!(vp.scene.edit_group(), Some(id));
}

/// A 3D text is a container, but its outlines are what the cursor carries.
#[test]
fn test_place_tool_previews_the_letter_outlines() {
    let mut vp = stub();
    let g = make_text_group(&mut vp.scene.doc.meshes, &TextParams::new("Ab", "Sans")).unwrap();
    let edges: usize = g
        .children
        .iter()
        .map(|k| vp.scene.doc.mesh(k.mesh).edges.len())
        .sum();
    let tool = PlaceGroupTool::new(&vp.scene.doc.meshes, g, true, None);
    assert!(edges > 12);
    assert_eq!(tool.segment_count(), edges);
}

/// Right-click on a letter while inside the text offers to edit the TEXT:
/// the container is what carries the parameters.
#[test]
fn test_edit_3d_text_reaches_the_container_from_a_letter_inside() {
    let mut win = window();
    let scene = &mut win.viewport.scene;
    let text = make_text_group(&mut scene.doc.meshes, &TextParams::new("Ab", "Sans")).unwrap();
    let (id, letter) = (text.id, text.children[1].id);
    scene.doc.groups.push(text);
    scene.begin_group_edit(id);
    scene.select([EntityRef::Group(letter)], SelectMode::Replace);
    assert_eq!(win.selected_text3d(), Some(id));
    let scene = &mut win.viewport.scene;
    scene.end_group_edit();
    scene.select([EntityRef::Group(id)], SelectMode::Replace);
    assert_eq!(win.selected_text3d(), Some(id));
    let scene = &mut win.viewport.scene;
    let bx = Group::new(add_mesh(&mut scene.doc.meshes, Mesh::new()), Some("box"));
    let box_id = bx.id;
    scene.doc.groups.push(bx);
    scene.select([EntityRef::Group(box_id)], SelectMode::Replace);
    assert_eq!(win.selected_text3d(), None);
}

fn touch_vertex(t: &mut MeshTable, letter: &Group) {
    let m = std::sync::Arc::make_mut(t.get_mut(&letter.mesh).unwrap());
    let v = m.verts.ids()[0];
    m.move_vertex(v, DVec3::new(0.0, 0.0, 0.01));
}

#[test]
fn test_edit_3d_text_refuses_a_text_edited_by_hand() {
    let mut win = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    let scene = &mut win.viewport.scene;
    let g = make_text_group(&mut scene.doc.meshes, &TextParams::new("Ab", "Sans")).unwrap();
    touch_vertex(&mut scene.doc.meshes, &g.children[0]);
    let id = g.id;
    scene.doc.groups.push(g);
    win.on_edit_3d_text(Some(id));
    let flashed = win
        .seen_events
        .iter()
        .any(|e| matches!(e, ViewportEvent::Flash(..)));
    let opened = dialogs.borrow().asked.iter().any(|(k, _)| k == "text3d");
    assert!(flashed && !opened);
}
