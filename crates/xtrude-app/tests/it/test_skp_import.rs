// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The `.skp` import through the window, end to end on a real file (one of the
//! OpenSKP fixtures): parsed on the worker, applied as one undo step, named in
//! the title.

use crate::common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use xtrude_app::window::ScriptedDialogs;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../openskp/tests/fixtures")
        .join(name)
}

/// A classic file with nothing drawn in it cannot be told apart from one
/// the reader failed on (no thumbnail before 2021): it is reported.
#[test]
fn an_empty_classic_skp_is_reported() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    assert!(!win.import_skp_path(&fixture("blank_v17.skp")));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].1.contains("COLLADA"));
}

#[test]
fn a_skp_imports_as_one_undo_step_and_names_the_window() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    assert!(win.import_skp_path(&fixture("capilla_quiroz_v17.skp")));
    assert!(
        dialogs.borrow().asked.is_empty(),
        "{:?}",
        dialogs.borrow().asked
    );
    let groups = win.viewport.scene.doc.groups.len();
    assert!(groups > 0);
    assert_eq!(win.import_name.as_deref(), Some("capilla_quiroz_v17.skp"));
    assert!(
        win.title.contains("capilla_quiroz_v17.skp"),
        "{}",
        win.title
    );

    win.viewport.undo();
    assert!(win.viewport.scene.doc.groups.is_empty());
    win.viewport.redo();
    assert_eq!(win.viewport.scene.doc.groups.len(), groups);
}

#[test]
fn open_path_and_the_command_line_route_a_skp_to_the_import() {
    let mut win = common::window();
    win.dialogs = Box::new(Rc::new(RefCell::new(ScriptedDialogs::default())));
    assert!(win.open_path(&fixture("capilla_quiroz_v17.skp")));
    assert_eq!(win.import_name.as_deref(), Some("capilla_quiroz_v17.skp"));
    assert_eq!(win.current_path, None); // imported, not opened as the document

    // From the command line the import waits for the first paint.
    let mut win = common::window();
    win.dialogs = Box::new(Rc::new(RefCell::new(ScriptedDialogs::default())));
    let skp = fixture("capilla_quiroz_v17.skp");
    win.open_on_start(&skp);
    assert_eq!(win.pending_import.as_deref(), Some(skp.as_path()));
    assert!(win.viewport.scene.doc.groups.is_empty());
    assert!(win.open_pending());
    assert!(win.pending_import.is_none());
    assert!(!win.viewport.scene.doc.groups.is_empty());
}
