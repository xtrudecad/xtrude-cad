// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Blender's Shift+R and right-click ▸ Repeat. A tool comes back as if its key
//! were pressed; a one-shot command runs again on the CURRENT selection. Only
//! on deliberate gestures — never Space or Enter.

use crate::common;

use common::{context_texts, v3, window};
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_app::window::{CtxItem, KeySeq, MainWindow};
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

fn face(w: &mut MainWindow, x0: f64) -> FaceId {
    w.viewport.scene.mesh_mut().add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 2., 0., 0.),
            v3(x0 + 2., 2., 0.),
            v3(x0, 2., 0.),
        ],
        &[],
    )
}

fn select(w: &mut MainWindow, f: FaceId) {
    w.viewport
        .scene
        .select([EntityRef::Face(MeshId::LOOSE, f)], SelectMode::Replace);
}

fn normal(w: &MainWindow, f: FaceId) -> glam::DVec3 {
    w.viewport.scene.mesh().face_normal(f)
}

fn repeat_text(w: &MainWindow) -> String {
    w.actions.get("repeat_last_command").unwrap().text.clone()
}

#[test]
fn test_nothing_to_repeat_on_a_new_document() {
    let mut w = window();
    assert!(!w.actions.get("repeat_last_command").unwrap().enabled);
    assert!(!w.repeat_last_command());
    assert_eq!(w.active_tool_key(), Some("select"));
}

#[test]
fn test_shift_r_is_the_only_owner_of_its_key() {
    let w = window();
    let owners: Vec<&str> = w
        .actions
        .owners(KeySeq::parse("Shift+R").unwrap())
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(owners, vec!["repeat_last_command"]);
}

#[test]
fn test_a_tool_comes_back_after_space() {
    let mut w = window();
    w.activate_tool("line");
    w.activate_tool("select"); // Space
    assert_eq!(repeat_text(&w), "Repeat Line");
    assert!(w.repeat_last_command());
    assert_eq!(w.active_tool_key(), Some("line"));
    assert!(w.is_checked("line"));
}

#[test]
fn test_select_and_context_tools_are_not_remembered() {
    let mut w = window();
    w.activate_tool("rectangle");
    w.activate_tool("select");
    w.activate_tool("change_axes");
    w.activate_tool("select");
    w.repeat_last_command();
    assert_eq!(w.active_tool_key(), Some("rectangle"));
}

#[test]
fn test_a_command_runs_again_on_the_new_selection() {
    let mut w = window();
    let (a, b) = (face(&mut w, 0.), face(&mut w, 5.));
    let (na, nb) = (normal(&w, a), normal(&w, b));
    select(&mut w, a);
    assert!(w.trigger("reverse_faces")); // the Edit menu entry
    assert!(normal(&w, a).dot(na) < -0.99);
    assert_eq!(repeat_text(&w), "Repeat Reverse Faces");
    select(&mut w, b);
    assert!(w.repeat_last_command());
    assert!(normal(&w, b).dot(nb) < -0.99);
    assert!(normal(&w, a).dot(na) < -0.99, "a is left alone");
}

fn ctx_entry(w: &MainWindow, text: &str) -> String {
    w.context_menu()
        .into_iter()
        .find_map(|i| match i {
            CtxItem::Entry { id, text: t, .. } if t == text => Some(id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {text:?} in the context menu"))
}

#[test]
fn test_the_right_click_entry_is_repeatable_after_its_menu_dies() {
    let mut w = window();
    let (a, b) = (face(&mut w, 0.), face(&mut w, 5.));
    let nb = normal(&w, b);
    select(&mut w, a);
    let id = ctx_entry(&w, "Reverse Faces"); // the menu is built afresh…
    w.trigger(&id); // …and gone after the click
    select(&mut w, b);
    w.repeat_last_command();
    assert!(normal(&w, b).dot(nb) < -0.99);
}

#[test]
fn test_intersect_repeats_with_the_same_mode() {
    let mut w = window();
    let f = face(&mut w, 0.);
    // a wall through the face's middle: there is something to intersect
    let wall = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(1., -1., -1.),
            v3(1., 3., -1.),
            v3(1., 3., 1.),
            v3(1., -1., 1.),
        ],
        &[],
    );
    w.viewport.scene.select(
        [
            EntityRef::Face(MeshId::LOOSE, f),
            EntityRef::Face(MeshId::LOOSE, wall),
        ],
        SelectMode::Replace,
    );
    w.trigger("intersect:selection");
    assert_eq!(
        w.last_command().map(|(_, r)| r.clone()),
        Some(xtrude_app::window::Repeat::Command(
            "intersect:selection".into()
        ))
    );
    w.repeat_last_command();
    // the same mode again
    assert_eq!(
        w.last_command().map(|(_, r)| r.clone()),
        Some(xtrude_app::window::Repeat::Command(
            "intersect:selection".into()
        ))
    );
}

#[test]
fn test_the_last_one_wins_tool_or_command() {
    let mut w = window();
    let f = face(&mut w, 0.);
    select(&mut w, f);
    w.activate_tool("move");
    w.activate_tool("select");
    w.trigger("reverse_faces");
    assert_eq!(repeat_text(&w), "Repeat Reverse Faces");
    w.activate_tool("circle");
    assert_eq!(repeat_text(&w), "Repeat Circle");
}

#[test]
fn test_enter_on_select_is_not_a_repeat() {
    let mut w = window();
    let f = face(&mut w, 0.);
    let n = normal(&w, f);
    select(&mut w, f);
    w.trigger("reverse_faces");
    w.activate_tool("line");
    w.activate_tool("select");
    w.key_press(&KeyEvent::new(Key::Return, "\r", Modifiers::NONE));
    assert_eq!(w.active_tool_key(), Some("select"));
    assert!(
        normal(&w, f).dot(n) < -0.99,
        "the reversed face was not reversed back"
    );
}

// ---- only what ran is remembered -----------------------------------------

fn last_id(w: &MainWindow) -> Option<String> {
    w.last_command().map(|(_, r)| match r {
        xtrude_app::window::Repeat::Command(id) | xtrude_app::window::Repeat::Tool(id) => {
            id.clone()
        }
    })
}

#[test]
fn test_a_refused_command_is_not_remembered() {
    let mut w = window();
    w.trigger("make_group"); // nothing selected: refused
    assert_eq!(w.last_command(), None);
    assert!(!w.actions.get("repeat_last_command").unwrap().enabled);
    let f = face(&mut w, 0.);
    select(&mut w, f);
    w.trigger("make_group");
    assert_eq!(last_id(&w).as_deref(), Some("make_group"));
    assert_eq!(repeat_text(&w), "Repeat Make Group");
}

#[test]
fn test_a_refusal_keeps_the_command_before() {
    let mut w = window();
    let f = face(&mut w, 0.);
    select(&mut w, f);
    w.trigger("reverse_faces");
    w.viewport.scene.selection.clear();
    for id in ["make_group", "make_component", "reverse_faces", "hide"] {
        w.trigger(id); // each refuses: nothing selected
    }
    w.activate_tool("select");
    assert_eq!(last_id(&w).as_deref(), Some("reverse_faces"));
    assert_eq!(repeat_text(&w), "Repeat Reverse Faces");
    assert_eq!(
        w.repeat_hint().as_deref(),
        Some("Shift+R: repeat Reverse Faces")
    );
}

#[test]
fn test_a_cancelled_dialog_is_not_remembered() {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    let (a, b) = (face(&mut w, 0.), face(&mut w, 5.));
    w.trigger("make_group");
    assert_eq!(w.last_command(), None, "refused: nothing selected");
    select(&mut w, a);
    w.trigger("reverse_faces");
    w.viewport.scene.select(
        [
            EntityRef::Face(MeshId::LOOSE, a),
            EntityRef::Face(MeshId::LOOSE, b),
        ],
        SelectMode::Replace,
    );
    w.trigger("make_group");
    assert_eq!(last_id(&w).as_deref(), Some("make_group"));
    let g = w.viewport.scene.doc.groups[0].id;
    let c = face(&mut w, 10.);
    // a group and a loose face: Make Component asks for a name
    w.viewport.scene.select(
        [EntityRef::Group(g), EntityRef::Face(MeshId::LOOSE, c)],
        SelectMode::Replace,
    );
    dialogs.borrow_mut().texts.push(None); // cancelled
    w.trigger("make_component");
    assert_eq!(last_id(&w).as_deref(), Some("make_group"));
    w.trigger("text3d"); // the 3D Text dialog, cancelled
    assert_eq!(last_id(&w).as_deref(), Some("make_group"));
    dialogs.borrow_mut().texts.push(Some("Lid".into()));
    w.trigger("make_component");
    assert_eq!(last_id(&w).as_deref(), Some("make_component"));
}

// ---- where you see it: the right-click menu and the status bar ----------

#[test]
fn test_right_click_offers_the_repeat_first() {
    let mut w = window();
    let (a, b) = (face(&mut w, 0.), face(&mut w, 5.));
    let nb = normal(&w, b);
    select(&mut w, a);
    w.trigger("reverse_faces");
    select(&mut w, b);
    let first = w.context_menu().into_iter().next().unwrap();
    assert_eq!(first.text(), "Repeat Reverse Faces");
    if let CtxItem::Entry { id, .. } = first {
        w.trigger(&id);
    }
    assert!(normal(&w, b).dot(nb) < -0.99);
}

#[test]
fn test_right_click_has_no_repeat_before_anything_was_done() {
    let w = window();
    assert!(!context_texts(&w).iter().any(|t| t.starts_with("Repeat")));
}

#[test]
fn test_the_status_bar_says_what_would_repeat_only_in_select() {
    let mut w = window();
    assert!(w.repeat_hint().is_none(), "nothing to repeat yet");
    w.activate_tool("line");
    assert!(
        w.repeat_hint().is_none(),
        "inside a tool the corner is the value box's"
    );
    w.activate_tool("select");
    assert_eq!(w.repeat_hint().as_deref(), Some("Shift+R: repeat Line"));
}

#[test]
fn test_the_hint_names_the_keys_it_has_now() {
    let mut w = window();
    w.activate_tool("line");
    w.activate_tool("select");
    w.actions.get_mut("repeat_last_command").unwrap().shortcuts =
        vec![KeySeq::parse("Ctrl+Shift+Y").unwrap()];
    assert_eq!(
        w.repeat_hint().as_deref(),
        Some("Ctrl+Shift+Y: repeat Line")
    );
    w.actions
        .get_mut("repeat_last_command")
        .unwrap()
        .shortcuts
        .clear();
    assert_eq!(w.repeat_hint().as_deref(), Some("Repeat Line"));
}
