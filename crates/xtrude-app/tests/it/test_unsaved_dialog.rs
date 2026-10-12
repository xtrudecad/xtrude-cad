// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The unsaved-changes question names its own three buttons, the same on every
//! platform, and Save is the default. (The egui shell draws every dialog
//! itself, so there is no native alert to keep away.)

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{add_box, v3, window};
use xtrude_app::window::dialogs::{unsaved_buttons, unsaved_title};
use xtrude_app::window::{MainWindow, SaveChoice, ScriptedDialogs};

/// A window with something unsaved, asking through `dialogs`.
fn dirty_window(dialogs: &Rc<RefCell<ScriptedDialogs>>) -> MainWindow {
    let mut win = window();
    win.dialogs = Box::new(dialogs.clone());
    add_box(&mut win.viewport, v3(0., 0., 0.), v3(1., 1., 1.));
    assert!(win.is_dirty());
    win
}

#[test]
fn test_the_buttons_read_the_same_on_every_platform() {
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().unsaved.push(SaveChoice::Cancel);
    let mut win = dirty_window(&dialogs);
    assert!(!win.confirm_discard("Quit Xtrude CAD?"));
    assert_eq!(dialogs.borrow().asked[0].0, "unsaved");
    let texts: Vec<String> = unsaved_buttons().into_iter().map(|(t, _)| t).collect();
    assert_eq!(texts, ["Save", "Don't Save", "Cancel"]);
}

/// The three buttons Save / Don't Save / Cancel, Save the default, and
/// Don't Save lets the drawing go.
#[test]
fn test_save_is_the_default_of_three_buttons() {
    let choices: Vec<SaveChoice> = unsaved_buttons().into_iter().map(|(_, c)| c).collect();
    assert_eq!(
        choices,
        [SaveChoice::Save, SaveChoice::Discard, SaveChoice::Cancel]
    );
    assert_eq!(SaveChoice::DEFAULT, SaveChoice::Save);
    assert_eq!(unsaved_title(), "Unsaved changes");
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().unsaved.push(SaveChoice::Discard);
    let mut win = dirty_window(&dialogs);
    assert!(win.confirm_discard("Quit Xtrude CAD?"));
}
