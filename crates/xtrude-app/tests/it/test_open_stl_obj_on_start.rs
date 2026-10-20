// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An STL or an OBJ given on the command line (a double-click in the file
//! manager) is imported into the new document. Both imports ask a question
//! first (the STL's unit and simplification, the OBJ's unit), so the
//! import waits for the first paint and runs as an asker: the shell shows
//! the question, and the answer replays the import.

use crate::common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::window;
use xtrude_app::shell::dialogs::{answer, dismiss, Answer, Question, ShellDialogs};
use xtrude_app::tray::Tray;
use xtrude_app::window::{Asker, MainWindow, ScriptedDialogs};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name)
}

fn face_count(w: &MainWindow) -> usize {
    let s = &w.viewport.scene;
    s.mesh().faces.len()
        + s.doc
            .groups
            .iter()
            .map(|g| s.doc.mesh(g.mesh).faces.len())
            .sum::<usize>()
}

/// A new window asking through the shell's answerer, `path` given on the
/// command line and the first paint done: the import's question is up.
fn started_with(path: &Path) -> (MainWindow, Rc<RefCell<ShellDialogs>>) {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ShellDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    w.open_on_start(path);
    // nothing before the first paint
    assert_eq!(w.pending_import.as_deref(), Some(path));
    assert!(dialogs.borrow().pending.is_none());
    assert!(!w.open_pending());
    assert!(w.pending_import.is_none());
    assert_eq!(face_count(&w), 0);
    assert_eq!(
        dialogs.borrow().asked_by,
        Some(Asker::Open(path.to_path_buf()))
    );
    (w, dialogs)
}

/// The imported file is in the window, as one undo step, and names it.
fn assert_imported(w: &MainWindow, dialogs: &RefCell<ShellDialogs>, name: &str) {
    assert!(face_count(w) > 0);
    assert!(w.viewport.history.can_undo());
    assert_eq!(w.import_name.as_deref(), Some(name));
    assert!(w.title.contains(name), "{}", w.title);
    assert!(w.current_path.is_none());
    let d = dialogs.borrow();
    assert!(d.pending.is_none());
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn test_a_start_up_stl_imports_once_its_options_are_answered() {
    let stl = golden("box.stl");
    let (mut w, dialogs) = started_with(&stl);
    assert!(matches!(
        dialogs.borrow().pending,
        Some(Question::Stl { .. })
    ));
    let mut tray = Tray::default();
    // millimetres, no simplification
    answer(&dialogs, Answer::Stl(Some((2, 0))), &mut w, &mut tray);
    assert_imported(&w, &dialogs, "box.stl");
}

#[test]
fn test_a_start_up_obj_imports_once_its_unit_is_answered() {
    let obj = golden("box.obj");
    let (mut w, dialogs) = started_with(&obj);
    assert!(matches!(
        dialogs.borrow().pending,
        Some(Question::Choose { .. })
    ));
    let mut tray = Tray::default();
    answer(&dialogs, Answer::Index(Some(0)), &mut w, &mut tray);
    assert_imported(&w, &dialogs, "box.obj");
}

#[test]
fn test_a_cancelled_start_up_question_leaves_the_empty_document() {
    for name in ["box.stl", "box.obj"] {
        let (mut w, dialogs) = started_with(&golden(name));
        let mut tray = Tray::default();
        dismiss(&dialogs, &mut w, &mut tray);
        assert_eq!(face_count(&w), 0);
        assert!(!w.viewport.history.can_undo());
        assert!(w.import_name.is_none());
        assert!(!w.is_dirty());
        let d = dialogs.borrow();
        assert!(d.pending.is_none());
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    }
}

#[test]
fn test_open_path_imports_an_stl_and_an_obj() {
    for (name, unit) in [("box.stl", false), ("box.obj", true)] {
        let mut w = window();
        let d = Rc::new(RefCell::new(ScriptedDialogs::default()));
        w.dialogs = Box::new(d.clone());
        if unit {
            d.borrow_mut().choices.push(Some(0));
        } else {
            d.borrow_mut().stl.push(Some((2, 0)));
        }
        assert!(w.open_path(&golden(name)), "{name}");
        assert!(face_count(&w) > 0, "{name}");
        assert!(w.viewport.history.can_undo(), "{name}");
        assert_eq!(w.import_name.as_deref(), Some(name));
        let d = d.borrow();
        assert!(
            d.asked.iter().all(|(kind, _)| kind != "warn"),
            "{:?}",
            d.asked
        );
    }
}

#[test]
fn test_open_path_with_the_question_cancelled_imports_nothing_and_warns_nothing() {
    for name in ["box.stl", "box.obj"] {
        let mut w = window();
        let d = Rc::new(RefCell::new(ScriptedDialogs::default()));
        w.dialogs = Box::new(d.clone());
        assert!(!w.open_path(&golden(name)), "{name}");
        assert_eq!(face_count(&w), 0, "{name}");
        let d = d.borrow();
        assert_eq!(d.asked.len(), 1, "{:?}", d.asked);
        assert!(
            d.asked.iter().all(|(kind, _)| kind != "warn"),
            "{:?}",
            d.asked
        );
    }
}
