// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Xtrude CAD saves its own documents (`.xtr`) and reads IngeTrazo's `.igz`
//! as an import: an `.igz` opens as a new, unsaved document with no path,
//! Save then asks where to write the `.xtr`, and the `.igz` on disk is
//! never written.

use crate::common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::{window, TmpDir};
use xtrude_app::window::files::{recent_paths, remember_recent};
use xtrude_app::window::{MainWindow, ScriptedDialogs};
use xtrude_io::document;

fn upstream() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/upstream_removed_features.igz")
}

/// A copy of an IngeTrazo document in `dir`, to open and compare.
fn igz_copy(dir: &TmpDir) -> PathBuf {
    let p = dir.path().join("servo.igz");
    std::fs::copy(upstream(), &p).unwrap();
    p
}

fn scripted(w: &mut MainWindow) -> Rc<RefCell<ScriptedDialogs>> {
    let d = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(d.clone());
    d
}

fn assert_opened_unsaved(w: &MainWindow) {
    assert_eq!(w.viewport.scene.mesh().faces.len(), 6);
    assert!(w.current_path.is_none(), "{:?}", w.current_path);
    assert!(w.is_dirty());
    assert!(
        w.title.contains("servo.igz") && w.title.ends_with(" *"),
        "{}",
        w.title
    );
}

#[test]
fn test_an_igz_opens_as_an_unsaved_document_without_a_path() {
    let dir = TmpDir::new();
    let igz = igz_copy(&dir);
    let mut w = window();
    assert!(w.open_path(&igz));
    assert_opened_unsaved(&w);
    assert!(
        recent_paths().is_empty(),
        "an import is not a recent document"
    );
}

#[test]
fn test_save_asks_for_a_path_and_writes_a_document_beside_the_untouched_igz() {
    let dir = TmpDir::new();
    let igz = igz_copy(&dir);
    let before = std::fs::read(&igz).unwrap();
    let mut w = window();
    assert!(w.open_path(&igz));
    let d = scripted(&mut w);
    let target = dir.path().join("servo.xtr");
    d.borrow_mut().save_paths.push(Some(target.clone()));
    w.trigger("save");
    let d = d.borrow();
    assert_eq!(d.asked[0].0, "save", "{:?}", d.asked);
    assert_eq!(d.starts[0].file_name().unwrap(), "servo.xtr");
    assert!(d.filters[0].starts_with("Xtrude CAD document (*.xtr)"));
    assert_eq!(w.current_path.as_deref(), Some(target.as_path()));
    assert!(!w.is_dirty());
    assert_eq!(std::fs::read(&igz).unwrap(), before);
    let mut back = xtrude_model::scene::Scene::new();
    document::load_into(&mut back, &target, None).unwrap();
    assert_eq!(back.mesh().faces.len(), 6);
}

#[test]
fn test_saving_under_an_igz_name_writes_a_document_instead() {
    let dir = TmpDir::new();
    let igz = igz_copy(&dir);
    let before = std::fs::read(&igz).unwrap();
    let mut w = window();
    assert!(w.open_path(&igz));
    let d = scripted(&mut w);
    d.borrow_mut().save_paths.push(Some(igz.clone()));
    w.trigger("save_as");
    assert_eq!(std::fs::read(&igz).unwrap(), before);
    let written = dir.path().join("servo.xtr");
    assert_eq!(w.current_path.as_deref(), Some(written.as_path()));
    assert!(std::fs::read(&written).unwrap().starts_with(b"\x1f\x8b"));
    // a saved document gives `.xtr` again, however it is named
    w.do_save(&written);
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".igz"))
        .collect();
    assert_eq!(names, ["servo.igz"]);
}

#[test]
fn test_an_igz_from_the_command_line_or_open_recent_opens_unsaved_too() {
    let dir = TmpDir::new();
    let igz = igz_copy(&dir);
    let mut w = window();
    w.open_on_start(&igz);
    assert_opened_unsaved(&w);
    // an `.igz` an older build listed in Open Recent
    let mut w = window();
    w.open_recent(&igz);
    assert_opened_unsaved(&w);
}

#[test]
fn test_a_document_opens_with_its_path_and_is_remembered() {
    let dir = TmpDir::new();
    let mut w = window();
    assert!(w.open_path(&igz_copy(&dir)));
    let path = dir.path().join("servo.xtr");
    w.do_save(&path);
    let mut w = window();
    assert!(w.open_path(&path));
    assert_eq!(w.current_path.as_deref(), Some(path.as_path()));
    assert!(!w.is_dirty());
    assert!(w.title.contains("servo.xtr") && !w.title.ends_with(" *"));
    let canon = path.canonicalize().unwrap().to_string_lossy().into_owned();
    assert_eq!(recent_paths().first(), Some(&canon));
    remember_recent(&igz_copy(&dir));
    assert_eq!(recent_paths(), [canon]);
}

#[test]
fn test_open_offers_both_documents_and_save_only_the_native_one() {
    let mut w = window();
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.push(None);
    w.trigger("open");
    d.borrow_mut().save_paths.push(None);
    w.trigger("save_as");
    let d = d.borrow();
    assert!(d.filters[0].starts_with("Xtrude CAD or IngeTrazo document (*.xtr *.igz)"));
    assert!(d.filters[1].starts_with("Xtrude CAD document (*.xtr)"));
    assert!(!d.filters[1].contains("igz"));
    assert_eq!(d.starts[1].file_name().unwrap(), "untitled.xtr");
}

#[test]
fn test_import_offers_both_documents_as_components() {
    let mut w = window();
    for (id, text) in [
        ("import:xtr", "Xtrude CAD document as component (.xtr)…"),
        ("import:igz", "IngeTrazo document as component (.igz)…"),
    ] {
        assert_eq!(w.actions.get(id).map(|a| a.text.as_str()), Some(text));
    }
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.extend([None, None]);
    w.trigger("import:xtr");
    w.trigger("import:igz");
    let d = d.borrow();
    assert_eq!(
        d.asked,
        [
            ("open".to_string(), "Import Xtrude CAD document".to_string()),
            ("open".to_string(), "Import IngeTrazo document".to_string()),
        ]
    );
    assert!(d.filters[0].starts_with("Xtrude CAD document (*.xtr)"));
    assert!(d.filters[1].starts_with("IngeTrazo document (*.igz)"));
}

#[test]
fn test_an_igz_imports_as_one_component() {
    let dir = TmpDir::new();
    let mut w = window();
    let n0 = w.viewport.scene.doc.groups.len();
    let comp = w
        .import_document_path(&igz_copy(&dir), Some(glam::DVec3::ZERO))
        .unwrap();
    assert!(comp.is_some());
    assert_eq!(w.viewport.scene.doc.groups.len(), n0 + 1);
    assert_eq!(w.viewport.scene.doc.groups.last().unwrap().name, "servo");
}

#[test]
fn test_the_desktop_dialogs_get_the_document_extensions() {
    use xtrude_app::shell::dialogs::parse_filter;
    use xtrude_app::window::files::{DOCUMENT_FILE_FILTER, IGZ_FILE_FILTER, OPEN_FILE_FILTER};
    let exts = |f: &str| parse_filter(f).map(|(_, e)| e);
    assert_eq!(
        exts(OPEN_FILE_FILTER),
        Some(vec!["xtr".into(), "igz".into()])
    );
    assert_eq!(exts(DOCUMENT_FILE_FILTER), Some(vec!["xtr".into()]));
    assert_eq!(exts(IGZ_FILE_FILTER), Some(vec!["igz".into()]));
}
