// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The shell answers a question a frame late, by running the code that
//! asked it again. The answer goes back to that code — a menu action, a
//! side panel's button, the name prompt after placing a section plane —
//! and never to the last menu action or key.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{add_box, v3, window};
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::shell::dialogs::{answer, Answer, Question, ShellDialogs};
use xtrude_app::tools::section::SectionPlaneTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::tray::{Tray, TrayAction};
use xtrude_app::window::dialogs::SectionName;
use xtrude_app::window::{Asker, MainWindow, SaveChoice};
use xtrude_model::layers::DEFAULT_LAYER;

/// A window with a box unsaved, asking through the shell's answerer.
fn dirty_window() -> (MainWindow, Rc<RefCell<ShellDialogs>>) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ShellDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    add_box(&mut w.viewport, v3(0., 0., 0.), v3(1., 1., 1.));
    assert!(w.is_dirty());
    (w, dialogs)
}

/// File › New, and Cancel in «Unsaved changes»: the last menu action.
fn new_then_cancel(w: &mut MainWindow, dialogs: &RefCell<ShellDialogs>) {
    w.trigger("new");
    assert!(matches!(
        dialogs.borrow().pending,
        Some(Question::Unsaved { .. })
    ));
    dialogs.borrow_mut().cancel();
}

/// The box `dirty_window` drew is still there.
fn has_box(w: &MainWindow) -> bool {
    !w.viewport.scene.mesh().face_ids().is_empty()
}

fn layer_names(w: &MainWindow) -> Vec<String> {
    w.viewport
        .scene
        .doc
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

#[test]
fn test_yes_to_layers_purge_purges_and_runs_nothing_else() {
    let (mut w, dialogs) = dirty_window();
    let mut tray = Tray::default();
    new_then_cancel(&mut w, &dialogs);
    tray.layers.add(&mut w);
    assert_eq!(layer_names(&w).len(), 2);
    tray.run(&mut w, TrayAction::PurgeLayers);
    assert!(matches!(
        dialogs.borrow().pending,
        Some(Question::Confirm { .. })
    ));
    assert_eq!(
        dialogs.borrow().asked_by,
        Some(Asker::Tray(TrayAction::PurgeLayers))
    );
    answer(&dialogs, Answer::Bool(true), &mut w, &mut tray);
    assert_eq!(layer_names(&w), [DEFAULT_LAYER]);
    // File › New did not run again: no «Unsaved changes», the box is kept
    assert!(dialogs.borrow().pending.is_none());
    assert!(has_box(&w));
    assert!(w.is_dirty());
}

#[test]
fn test_the_section_name_goes_to_the_plane_just_placed() {
    let (mut w, dialogs) = dirty_window();
    let mut tray = Tray::default();
    new_then_cancel(&mut w, &dialogs);
    let mut tool = SectionPlaneTool::new();
    tool.set_normal(v3(0., 0., 1.));
    tool.on_click(&mut w.viewport, &ToolContext::at(v3(0., 0., 1.)));
    w.pump_events();
    let id = w.viewport.scene.doc.section_planes[0].id;
    assert!(matches!(
        dialogs.borrow().pending,
        Some(Question::SectionName { .. })
    ));
    assert_eq!(dialogs.borrow().asked_by, Some(Asker::SectionName(id)));
    let named = SectionName {
        name: "Front".into(),
        symbol: "F".into(),
        dont_ask: false,
    };
    answer(&dialogs, Answer::Section(Some(named)), &mut w, &mut tray);
    let plane = &w.viewport.scene.doc.section_planes[0];
    assert_eq!((plane.name.as_str(), plane.symbol.as_str()), ("Front", "F"));
    assert!(dialogs.borrow().pending.is_none());
    assert!(w.is_dirty());
}

#[test]
fn test_a_menu_actions_answer_still_runs_it() {
    let (mut w, dialogs) = dirty_window();
    let mut tray = Tray::default();
    w.trigger("new");
    assert_eq!(
        dialogs.borrow().asked_by,
        Some(Asker::Trigger("new".into()))
    );
    answer(
        &dialogs,
        Answer::Save(SaveChoice::Discard),
        &mut w,
        &mut tray,
    );
    assert!(!has_box(&w));
    assert!(!w.is_dirty());
}

#[test]
fn test_a_shortcut_asks_as_its_action() {
    let (mut w, dialogs) = dirty_window();
    let mut tray = Tray::default();
    w.key_press(&KeyEvent::new(Key::Char('N'), "n", Modifiers::CTRL));
    assert_eq!(
        dialogs.borrow().asked_by,
        Some(Asker::Trigger("new".into()))
    );
    answer(
        &dialogs,
        Answer::Save(SaveChoice::Discard),
        &mut w,
        &mut tray,
    );
    assert!(!has_box(&w));
}

#[test]
fn test_a_question_no_one_can_replay_is_dropped() {
    let (mut w, dialogs) = dirty_window();
    let mut tray = Tray::default();
    new_then_cancel(&mut w, &dialogs);
    // asked outside any asker
    assert!(!w.dialogs.confirm("Purge", "Delete?"));
    assert!(dialogs.borrow().pending.is_some());
    assert_eq!(dialogs.borrow().asked_by, None);
    answer(&dialogs, Answer::Bool(true), &mut w, &mut tray);
    let d = dialogs.borrow();
    assert!(d.pending.is_none());
    assert!(d.answers.is_empty());
    assert!(has_box(&w));
}
