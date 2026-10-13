// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A control that asks two questions in a row, through the shell's
//! answerer: each question is asked once, and the action is applied once,
//! with every answer, when the last one comes back. Colors ▸ "+ Color…"
//! asks for a colour, then for an optional material name; Import OBJ picks
//! a file in the desktop's dialog, then asks for the unit.

use crate::common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use common::window;
use xtrude_app::shell::dialogs::{answer, Answer, FilePick, Question, ShellDialogs};
use xtrude_app::tools::paint::{paint_state, set_paint_state, PaintState};
use xtrude_app::tray::{Tray, TrayAction};
use xtrude_app::window::MainWindow;

const RED: [f64; 3] = [1.0, 0.0, 0.0];

fn shell_window() -> (MainWindow, Rc<RefCell<ShellDialogs>>, Tray) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ShellDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    set_paint_state(PaintState::default());
    (w, dialogs, Tray::default())
}

fn pending(dialogs: &RefCell<ShellDialogs>) -> Option<Question> {
    dialogs.borrow().pending.clone()
}

/// "+ Color…", and a colour answered: the name question is up, and
/// nothing is applied yet.
fn colour_answered(w: &mut MainWindow, dialogs: &RefCell<ShellDialogs>, tray: &mut Tray) {
    tray.run(w, TrayAction::AddColor);
    assert!(matches!(pending(dialogs), Some(Question::Color { .. })));
    answer(dialogs, Answer::Color(Some(RED)), w, tray);
    assert!(
        matches!(pending(dialogs), Some(Question::Text { .. })),
        "{:?}",
        pending(dialogs)
    );
    assert_ne!(paint_state().color, RED, "applied before the name");
    assert_ne!(w.active_tool_key(), Some("paint"));
}

#[test]
fn test_the_colour_and_the_name_are_asked_once_and_applied_together() {
    let (mut w, dialogs, mut tray) = shell_window();
    colour_answered(&mut w, &dialogs, &mut tray);
    answer(
        &dialogs,
        Answer::Text(Some("Brick".into())),
        &mut w,
        &mut tray,
    );
    // no second colour question
    assert!(pending(&dialogs).is_none(), "{:?}", pending(&dialogs));
    assert!(dialogs.borrow().answers.is_empty());
    let st = paint_state();
    assert_eq!(st.color, RED);
    assert_eq!(st.named_color.map(|m| m.name), Some("Brick".to_string()));
    assert_eq!(w.active_tool_key(), Some("paint"));
    assert!(!w.viewport.history.can_undo());
}

#[test]
fn test_cancel_on_the_name_applies_the_colour_unnamed() {
    let (mut w, dialogs, mut tray) = shell_window();
    colour_answered(&mut w, &dialogs, &mut tray);
    xtrude_app::shell::dialogs::dismiss(&dialogs, &mut w, &mut tray);
    assert!(pending(&dialogs).is_none(), "{:?}", pending(&dialogs));
    assert!(dialogs.borrow().answers.is_empty());
    let st = paint_state();
    assert_eq!(st.color, RED);
    assert!(st.named_color.is_none());
    assert_eq!(w.active_tool_key(), Some("paint"));
}

#[test]
fn test_cancel_on_the_colour_applies_nothing() {
    let (mut w, dialogs, mut tray) = shell_window();
    tray.run(&mut w, TrayAction::AddColor);
    assert!(matches!(pending(&dialogs), Some(Question::Color { .. })));
    xtrude_app::shell::dialogs::dismiss(&dialogs, &mut w, &mut tray);
    assert!(pending(&dialogs).is_none());
    assert!(dialogs.borrow().answers.is_empty());
    let (st, untouched) = (paint_state(), PaintState::default());
    assert_eq!(
        (st.color, st.named_color.is_none()),
        (untouched.color, true)
    );
    assert_ne!(w.active_tool_key(), Some("paint"));
    assert!(!w.viewport.history.can_undo());
}

#[test]
fn test_a_new_question_after_a_cancelled_chain_starts_afresh() {
    let (mut w, dialogs, mut tray) = shell_window();
    colour_answered(&mut w, &dialogs, &mut tray);
    dialogs.borrow_mut().cancel();
    assert!(dialogs.borrow().answers.is_empty());
    // the next "+ Color…" asks for the colour again
    tray.run(&mut w, TrayAction::AddColor);
    assert!(matches!(pending(&dialogs), Some(Question::Color { .. })));
}

/// Import OBJ: the desktop's file dialog, then the unit question. The
/// file is picked once; the unit's replay imports that file.
#[test]
fn test_the_file_picked_before_a_question_is_not_asked_for_again() {
    let (mut w, dialogs, mut tray) = shell_window();
    let picks = Rc::new(RefCell::new(Vec::new()));
    let seen = picks.clone();
    let obj = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/box.obj");
    dialogs.borrow_mut().files = Box::new(move |pick: &FilePick| {
        seen.borrow_mut().push(pick.clone());
        Some(obj.clone())
    });
    w.trigger("import");
    assert!(matches!(pending(&dialogs), Some(Question::Choose { .. })));
    answer(&dialogs, Answer::Index(Some(0)), &mut w, &mut tray);
    assert_eq!(picks.borrow().len(), 1, "{:?}", picks.borrow());
    assert!(!picks.borrow()[0].save);
    assert!(pending(&dialogs).is_none(), "{:?}", pending(&dialogs));
    assert!(dialogs.borrow().answers.is_empty());
    assert!(
        !w.viewport.scene.doc.groups.is_empty() || !w.viewport.scene.mesh().face_ids().is_empty()
    );
    assert!(w.viewport.history.can_undo());
}
