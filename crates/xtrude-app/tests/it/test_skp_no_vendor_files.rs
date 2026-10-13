// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A `.skp` the built-in reader cannot open is reported — no converter runs,
//! nothing is offered or downloaded — and there is no `.skp` export.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::TmpDir;
use xtrude_app::window::ScriptedDialogs;

/// The window has no converter code and runs no external program; what
/// is checked is the
/// behaviour: one warning that mentions COLLADA, and no question at all
/// (installing or running something would have to ask).
#[test]
fn test_an_unreadable_skp_is_reported_and_nothing_else_runs() {
    let tmp = TmpDir::new();
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    let bad = tmp.join("broken.skp");
    std::fs::write(&bad, b"not a .skp file at all").unwrap();
    assert!(!win.import_skp_path(&bad));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, "warn");
    assert!(asked[0].1.contains("COLLADA"), "{asked:?}");
}

#[test]
fn test_no_skp_export_in_the_menu() {
    use xtrude_app::window::files::{formats_filter, EXPORT_FORMATS};
    let win = common::window();
    let mut texts = vec![formats_filter(EXPORT_FORMATS, false)];
    let a = win.actions.get("export").expect("the export action");
    texts.push(format!("{} {}", a.text, a.status_tip));
    assert!(
        texts
            .iter()
            .all(|t| !t.contains("skp") && !t.contains("SketchUp")),
        "{texts:?}"
    );
    assert!(!win.actions.iter().any(|a| a.id.starts_with("export:")));
}
