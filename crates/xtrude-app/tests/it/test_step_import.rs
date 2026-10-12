// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File › Import › STEP through the window: the menu entry and its file
//! filter, the import as one undo step named in the title, the report of
//! what the import left out, a missing helper, Cancel, the routing of
//! `.step`/`.stp` from Open and the command line, and the tessellation
//! setting reaching the helper.
//!
//! Most tests run a stand-in helper (a shell script writing a prepared
//! stream); one reads a real file with the helper built with OpenCascade
//! and skips without it.

use crate::common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use xtrude_app::window::{MainWindow, MenuItem, ScriptedDialogs};
use xtrude_core::math::DMat4;
use xtrude_core::DVec3;
use xtrude_io::step::protocol::{encode, RawFace, RawInstance, RawPart, RawStep};

type Shared = Rc<RefCell<ScriptedDialogs>>;

fn window() -> (MainWindow, Shared) {
    let mut win = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    (win, dialogs)
}

/// A STEP fixture of the repository (`tests/data/step`).
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name)
}

/// The helper built with OpenCascade in the workspace's `target/`, if any.
fn real_helper() -> Option<PathBuf> {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let found = ["release", "debug"]
        .iter()
        .map(|p| {
            target
                .join(p)
                .join(xtrude_io::step::helper::helper_file_name())
        })
        .find(|p| {
            Command::new(p)
                .arg("--version")
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("(OpenCascade"))
        });
    if found.is_none() {
        eprintln!("skipped: no xtrude-step built with OpenCascade");
    }
    found
}

/// A 10 mm cube's faces, outward; the last `missing` left out.
fn cube(missing: usize) -> Vec<RawFace> {
    let s = 0.01;
    let quads = [
        [[0., 0., 0.], [0., s, 0.], [s, s, 0.], [s, 0., 0.]],
        [[0., 0., s], [s, 0., s], [s, s, s], [0., s, s]],
        [[0., 0., 0.], [s, 0., 0.], [s, 0., s], [0., 0., s]],
        [[0., s, 0.], [0., s, s], [s, s, s], [s, s, 0.]],
        [[0., 0., 0.], [0., 0., s], [0., s, s], [0., s, 0.]],
        [[s, 0., 0.], [s, s, 0.], [s, s, s], [s, 0., s]],
    ];
    quads[..6 - missing]
        .iter()
        .map(|q| RawFace {
            positions: q.iter().map(|&c| DVec3::from(c)).collect(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            planar: true,
            color: None,
        })
        .collect()
}

/// One part, placed once at the top level.
fn one_part(name: &str, faces: Vec<RawFace>, closed: bool) -> RawStep {
    RawStep {
        parts: vec![RawPart {
            name: name.into(),
            faces,
            closed,
            ..Default::default()
        }],
        roots: vec![RawInstance {
            part: 0,
            name: name.into(),
            xform: DMat4::IDENTITY,
        }],
    }
}

#[cfg(unix)]
mod stand_in {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    /// A helper script with `body` in `dir`.
    pub fn script(dir: &common::TmpDir, body: &str) -> PathBuf {
        let path = dir.join("xtrude-step");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A helper that writes `raw`'s stream, and records its arguments in
    /// `args.txt` next to it.
    pub fn writing(dir: &common::TmpDir, raw: &RawStep) -> PathBuf {
        let stream = dir.join("stream.bin");
        std::fs::write(&stream, encode(raw)).unwrap();
        let args = dir.join("args.txt");
        script(
            dir,
            &format!(
                "echo \"$@\" > '{}'\necho '@progress 0.5 Tessellating…' >&2\ncat '{}'",
                args.display(),
                stream.display()
            ),
        )
    }

    /// Run `import`; again when a script just written was still busy
    /// (another test thread starting a process can hold it an instant).
    pub fn retrying(
        win: &mut MainWindow,
        dialogs: &Shared,
        import: impl Fn(&mut MainWindow) -> bool,
    ) -> bool {
        for _ in 0..20 {
            let ok = import(win);
            let busy = dialogs
                .borrow()
                .asked
                .last()
                .is_some_and(|(_, text)| text.contains("busy"));
            if !busy {
                return ok;
            }
            dialogs.borrow_mut().asked.pop();
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the stand-in helper stayed busy");
    }
}

#[cfg(unix)]
use stand_in::{retrying, script, writing};

#[test]
fn test_the_import_menu_offers_step() {
    let (win, _) = window();
    let mut imports = Vec::new();
    for (_, items) in &win.menus {
        for item in items {
            if let MenuItem::Menu(title, sub) = item {
                if title.replace('&', "") == "Import" {
                    imports.extend(sub.iter().filter_map(|i| match i {
                        MenuItem::Action(id) => Some(id.clone()),
                        _ => None,
                    }));
                }
            }
        }
    }
    assert!(imports.contains(&"import:step".to_string()), "{imports:?}");
    let action = win.actions.get("import:step").expect("the action");
    assert!(action.text.contains(".step") && action.text.contains(".stp"));
}

#[test]
fn test_a_real_step_file_imports_as_one_undo_step() {
    let Some(helper) = real_helper() else { return };
    let (mut win, dialogs) = window();
    win.step_helper = Some(helper);
    assert!(win.import_step_path(&fixture("assembly_colours.step")));
    assert!(
        dialogs.borrow().asked.is_empty(),
        "{:?}",
        dialogs.borrow().asked
    );

    let groups = &win.viewport.scene.doc.groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Assembly");
    let names: Vec<&str> = groups[0].children.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["BracketLeft", "BracketRight", "Base"]);
    assert_eq!(win.import_name.as_deref(), Some("assembly_colours.step"));
    assert!(win.title.contains("assembly_colours.step"), "{}", win.title);
    assert_eq!(win.current_path, None, "imported, not opened");

    win.viewport.undo();
    assert!(win.viewport.scene.doc.groups.is_empty());
    win.viewport.redo();
    assert_eq!(win.viewport.scene.doc.groups.len(), 1);
}

#[cfg(unix)]
#[test]
fn test_the_menu_asks_for_a_step_file_and_imports_it() {
    let dir = common::TmpDir::new();
    let (mut win, dialogs) = window();
    win.step_helper = Some(writing(&dir, &one_part("Cube", cube(0), true)));
    let file = dir.join("cube.step");
    std::fs::write(&file, "ISO-10303-21;").unwrap();
    let answers = dialogs.clone();
    let ok = retrying(&mut win, &dialogs, |w| {
        answers.borrow_mut().open_paths = vec![Some(file.clone())];
        w.trigger("import:step")
    });
    assert!(ok);
    assert!(dialogs
        .borrow()
        .asked
        .iter()
        .any(|(kind, title)| kind == "open" && title == "Import STEP"));
    assert_eq!(win.viewport.scene.doc.groups.len(), 1);
    assert_eq!(win.viewport.scene.doc.groups[0].name, "Cube");
    let message = win.status_message.as_ref().map(|m| m.text.clone());
    assert_eq!(message.as_deref(), Some("Imported cube.step"));
}

#[cfg(unix)]
#[test]
fn test_what_the_import_left_out_is_reported() {
    let dir = common::TmpDir::new();
    let (mut win, dialogs) = window();
    win.step_helper = Some(writing(&dir, &one_part("Lid", cube(1), false)));
    let file = dir.join("lid.stp");
    let ok = retrying(&mut win, &dialogs, |w| w.import_step_path(&file));
    assert!(ok);
    assert_eq!(win.viewport.scene.doc.groups.len(), 1, "imported anyway");
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, "report");
    assert!(asked[0].1.starts_with("Imported lid.stp:"), "{asked:?}");
    assert!(asked[0].1.contains("“Lid” is not closed"), "{asked:?}");
    win.viewport.undo();
    assert!(win.viewport.scene.doc.groups.is_empty(), "one undo step");
}

#[test]
fn test_without_the_helper_step_support_is_not_installed() {
    let dir = common::TmpDir::new();
    let (mut win, dialogs) = window();
    win.step_helper = Some(dir.join("no-xtrude-step-here"));
    assert!(!win.import_step_path(&fixture("cube.step")));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(
        asked,
        [(
            "warn".to_string(),
            "Import STEP failed: STEP support is not installed.".to_string()
        )]
    );
    assert!(win.viewport.scene.doc.groups.is_empty());
    assert!(!win.viewport.history.can_undo(), "no undo step");
    assert!(win.progress.is_none());
}

#[cfg(unix)]
#[test]
fn test_cancel_stops_the_import_and_leaves_the_document_alone() {
    let dir = common::TmpDir::new();
    let file = dir.join("big.step");
    let helper = script(&dir, "exec sleep 30");
    for _ in 0..20 {
        let (mut win, dialogs) = window();
        win.background_imports = true;
        win.step_helper = Some(helper.clone());
        let started = Instant::now();
        assert!(win.import_step_path(&file), "under way");
        assert!(win.import_job.as_ref().is_some_and(|j| j.can_cancel()));
        win.cancel_import();
        assert_eq!(
            win.progress.as_ref().map(|p| p.label.as_str()),
            Some("Cancelling…")
        );
        let ok = loop {
            if let Some(ok) = win.poll_import() {
                break ok;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "Cancel did not stop it"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(!ok);
        let asked = dialogs.borrow().asked.clone();
        // The script just written can be busy an instant: try again.
        if asked.iter().any(|(_, text)| text.contains("busy")) {
            continue;
        }
        assert!(
            asked.is_empty(),
            "a cancel is no failure to warn about: {asked:?}"
        );
        let message = win.status_message.as_ref().map(|m| m.text.clone());
        assert_eq!(message.as_deref(), Some("Import of big.step cancelled"));
        assert!(win.progress.is_none());
        assert!(win.viewport.scene.doc.groups.is_empty());
        assert!(!win.viewport.history.can_undo());
        return;
    }
    panic!("the stand-in helper stayed busy");
}

#[test]
fn test_other_imports_offer_no_cancel() {
    let dir = common::TmpDir::new();
    let path = dir.join("one.stl");
    let mut bytes = vec![0u8; 80];
    bytes.extend(0u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    let job = xtrude_app::window::progress::ImportJob::stl(
        &path,
        0.001,
        xtrude_io::stl::SimplifyMode::None,
    )
    .unwrap();
    assert!(!job.can_cancel());
    job.cancel();
    assert!(!job.is_cancelled());
}

#[cfg(unix)]
#[test]
fn test_open_and_the_command_line_route_step_files_to_the_import() {
    let dir = common::TmpDir::new();
    let raw = one_part("Cube", cube(0), true);
    let file = dir.join("part.stp");

    let (mut win, dialogs) = window();
    win.step_helper = Some(writing(&dir, &raw));
    assert!(retrying(&mut win, &dialogs, |w| w.open_path(&file)));
    assert_eq!(win.import_name.as_deref(), Some("part.stp"));
    assert_eq!(win.current_path, None);

    // From the command line the import waits for the first paint.
    let (mut win, dialogs) = window();
    win.step_helper = Some(writing(&dir, &raw));
    win.open_on_start(&file);
    assert_eq!(win.pending_import.as_deref(), Some(file.as_path()));
    assert!(win.viewport.scene.doc.groups.is_empty());
    let ok = retrying(&mut win, &dialogs, |w| {
        if w.pending_import.is_none() {
            w.pending_import = Some(file.clone());
        }
        w.open_pending()
    });
    assert!(ok);
    assert!(win.pending_import.is_none());
    assert_eq!(win.viewport.scene.doc.groups.len(), 1);
}

#[cfg(unix)]
#[test]
fn test_the_tessellation_setting_reaches_the_helper() {
    use xtrude_model::settings::{self, keys, JsonText};

    let dir = common::TmpDir::new();
    let (mut win, dialogs) = window();
    settings::write(
        &keys::STEP_TESSELLATION,
        &JsonText(serde_json::json!({ "chord_mm": 0.05, "angle_deg": 15 })),
    );
    win.step_helper = Some(writing(&dir, &one_part("Cube", cube(0), true)));
    let file = dir.join("cube.step");
    assert!(retrying(&mut win, &dialogs, |w| w.import_step_path(&file)));
    let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
    assert_eq!(
        args.trim(),
        format!("{} --chord-mm 0.05 --angle-deg 15", file.display())
    );
}
