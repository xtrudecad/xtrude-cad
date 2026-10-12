// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File dialogs start in the last folder the user chose, not in the folder the
//! program is installed in. Every picker goes through
//! `MainWindow::pick_open` / `pick_save`, which hand the `Dialogs` a start
//! path and remember the pick.

use crate::common;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use common::TmpDir;
use xtrude_app::window::filedialogs::{last_dir, remember, start_in};
use xtrude_app::window::ScriptedDialogs;
use xtrude_model::settings;

fn fresh() {
    settings::use_memory_store();
    settings::remove("files/last_dir");
}

#[test]
fn test_nothing_remembered_means_documents_or_a_fallback() {
    fresh();
    let tmp = TmpDir::new();
    assert!(last_dir(None).is_dir());
    assert_eq!(last_dir(Some(tmp.path())), tmp.path()); // the open document's folder
    assert_eq!(start_in("", Some(tmp.path())), tmp.path());
    assert_eq!(
        start_in("sheet.pdf", Some(tmp.path())),
        tmp.join("sheet.pdf")
    );
}

#[test]
fn test_the_chosen_folder_is_remembered_and_reused() {
    fresh();
    let tmp = TmpDir::new();
    let chosen = tmp.join("drawings").join("sheet.pdf");
    std::fs::create_dir_all(chosen.parent().unwrap()).unwrap();
    remember(Some(&chosen));
    assert_eq!(last_dir(None), chosen.parent().unwrap());
    assert_eq!(start_in("", None), chosen.parent().unwrap());
    assert_eq!(
        start_in("vista.dxf", None),
        chosen.parent().unwrap().join("vista.dxf")
    );
    // a start directory the caller chose is respected
    let other = tmp.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let other_s = other.to_string_lossy().into_owned();
    assert_eq!(start_in(&other_s, None), other);
    let inside = other.join("x.igz");
    assert_eq!(start_in(&inside.to_string_lossy(), None), inside);
    // a remembered folder that vanished is not used
    settings::set(
        "files/last_dir",
        serde_json::json!(tmp.join("deleted").to_string_lossy()),
    );
    assert_eq!(last_dir(Some(tmp.path())), tmp.path());
}

#[test]
fn test_the_shim_feeds_the_dialog_and_learns_from_it() {
    fresh();
    let tmp = TmpDir::new();
    std::fs::create_dir_all(tmp.join("output")).unwrap();
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    remember(Some(tmp.path()));
    dialogs
        .borrow_mut()
        .save_paths
        .push(Some(tmp.join("output").join("sheet.pdf")));
    win.pick_save("Export PDF…", "PDF (*.pdf)", "sheet.pdf");
    assert_eq!(dialogs.borrow().starts[0], tmp.join("sheet.pdf")); // started where the user was
    assert_eq!(last_dir(None), tmp.join("output")); // …and learnt the new folder
    dialogs.borrow_mut().open_paths.push(None); // cancelled
    win.pick_open("Open", "IGZ (*.igz)");
    assert_eq!(dialogs.borrow().starts[1], tmp.join("output"));
    assert_eq!(last_dir(None), tmp.join("output")); // a cancel changes nothing
}

#[test]
fn test_with_nothing_remembered_a_dialog_starts_beside_the_open_document() {
    fresh();
    let tmp = TmpDir::new();
    std::fs::create_dir_all(tmp.join("project")).unwrap();
    let mut win = common::window();
    win.current_path = Some(tmp.join("project").join("peg.igz"));
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    dialogs.borrow_mut().save_paths.push(None);
    win.pick_save("Export PDF…", "PDF (*.pdf)", "sheet.pdf");
    let start = dialogs.borrow().starts[0].clone();
    assert_eq!(start, tmp.join("project").join("sheet.pdf"));
    assert!(Path::new(&start).parent().is_some_and(Path::is_dir));
}
