// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The import progress dialog's model: the bar and label the loaders'
//! milestones set, the 400 ms before it shows, one dialog at a time, and the
//! `.skp` and STL imports feeding it from their workers.

use crate::common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::TmpDir;
use xtrude_app::window::progress::{ImportJob, ImportProgress, ImportResult, MINIMUM_DURATION};
use xtrude_app::window::{MainWindow, ScriptedDialogs};
use xtrude_io::stl::SimplifyMode;

fn skp_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../openskp/tests/fixtures")
        .join(name)
}

/// A binary STL of `n` triangles.
fn write_stl(path: &Path, n: u32) {
    let mut bytes = b"binary STL".to_vec();
    bytes.resize(80, 0);
    bytes.extend(n.to_le_bytes());
    for i in 0..n {
        let x = i as f32;
        for f in [0f32, 0., 1., x, 0., 0., x + 1., 0., 0., x, 1., 0.] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.extend([0u8, 0]);
    }
    std::fs::write(path, bytes).unwrap();
}

/// Poll a background import as the shell does, a frame at a time; the
/// dialog stays up until the import is applied.
fn poll_to_end(win: &mut MainWindow) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(ok) = win.poll_import() {
            return ok;
        }
        assert!(win.progress.is_some(), "the dialog closed mid-import");
        assert!(Instant::now() < deadline, "the import never finished");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Every milestone a job delivers on the polling thread, in order.
fn milestones(mut job: ImportJob) -> (Vec<(f64, String)>, ImportResult) {
    let ui = std::thread::current().id();
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let done = job.poll(&mut |f, t| {
            assert_eq!(std::thread::current().id(), ui);
            seen.push((f, t.to_string()));
        });
        if let Some(r) = done {
            return (seen, r);
        }
        assert!(Instant::now() < deadline, "the parse never finished");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_workers_milestones_reach_the_polling_thread_in_order() {
    let tmp = TmpDir::new();
    let path = tmp.join("strip.stl");
    write_stl(&path, 200);
    let (seen, result) = milestones(ImportJob::stl(&path, 0.001, SimplifyMode::None).unwrap());
    assert!(matches!(result, ImportResult::Stl(Ok(_))));
    assert!(seen.iter().any(|(_, t)| t == "Reading file…"), "{seen:?}");
    assert_eq!(
        seen.last().map(|(f, t)| (*f, t.as_str())),
        Some((1.0, "Done"))
    );
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");

    let (seen, result) =
        milestones(ImportJob::skp(&skp_fixture("capilla_quiroz_v17.skp")).unwrap());
    assert!(matches!(result, ImportResult::Skp(Ok(_))));
    assert!(seen.len() > 1, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
}

#[test]
fn the_milestones_set_the_bar_and_the_label() {
    let mut p = ImportProgress::new("Importing part.stl…");
    assert_eq!(p.value, 0);
    assert_eq!(p.label, "Importing part.stl…");
    p.report(0.456, "Reading file…");
    assert_eq!(p.value, 45); // int(frac * 100)
    assert_eq!(p.label, "Reading file…");
    p.report(1.0, "Done");
    assert_eq!((p.value, p.label.as_str()), (100, "Done")); // no auto-close
}

#[test]
fn the_dialog_waits_400_ms_before_showing() {
    let p = ImportProgress::new("Opening big.igz…");
    assert_eq!(MINIMUM_DURATION, Duration::from_millis(400));
    assert!(!p.shown_at(Instant::now()));
    assert!(!p.shown_at(Instant::now() + Duration::from_millis(100)));
    assert!(p.shown_at(Instant::now() + MINIMUM_DURATION));
}

#[test]
fn only_one_dialog_is_alive() {
    let mut win = common::window();
    assert!(win.progress.is_none());
    win.import_progress("Importing a.skp…");
    if let Some(p) = win.progress.as_mut() {
        p.report(0.5, "Reading geometry…");
    }
    // the previous one goes when a new one is asked for
    win.import_progress("Importing b.stl…");
    let p = win.progress.as_ref().expect("a dialog");
    assert_eq!((p.value, p.label.as_str()), (0, "Importing b.stl…"));
    win.close_progress();
    assert!(win.progress.is_none());
}

#[test]
fn an_stl_import_keeps_the_dialog_up_until_applied() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    win.background_imports = true;
    let tmp = TmpDir::new();
    let path = tmp.join("strip.stl");
    write_stl(&path, 200);
    dialogs.borrow_mut().stl.push(Some((2, 0))); // millimetres, no simplification
    win.import_stl_path(&path);
    // under way: the dialog is up and nothing is applied yet
    assert!(win.import_job.is_some());
    assert_eq!(
        win.progress.as_ref().map(|p| p.label.as_str()),
        Some("Importing strip.stl…")
    );
    assert!(poll_to_end(&mut win));
    assert!(win.progress.is_none());
    assert!(win.import_job.is_none());
    assert_eq!(win.viewport.scene.mesh().face_ids().len(), 200); // a small mesh merges into the model
    assert_eq!(win.import_name.as_deref(), Some("strip.stl"));
}

#[test]
fn a_skp_import_keeps_the_dialog_up_until_applied() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    win.background_imports = true;
    assert!(win.import_skp_path(&skp_fixture("capilla_quiroz_v17.skp")));
    assert!(win.import_job.is_some());
    assert!(poll_to_end(&mut win));
    assert!(win.progress.is_none());
    assert!(!win.viewport.scene.doc.groups.is_empty());
    assert!(dialogs.borrow().asked.is_empty());
}

#[test]
fn a_failed_import_closes_the_dialog_before_saying_so() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    win.background_imports = true;
    // a classic file with nothing drawn: the reader cannot tell it from
    // a failure, so it is reported once parsed
    assert!(win.import_skp_path(&skp_fixture("blank_v17.skp")));
    assert!(!poll_to_end(&mut win));
    assert!(win.progress.is_none());
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].1.contains("COLLADA"), "{asked:?}");
}

#[test]
fn a_waited_import_leaves_no_dialog_behind() {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    assert!(!win.background_imports); // tests and scripts wait
    assert!(win.import_skp_path(&skp_fixture("capilla_quiroz_v17.skp")));
    assert!(win.import_job.is_none());
    assert!(win.progress.is_none());
    assert!(!win.viewport.scene.doc.groups.is_empty());
}
