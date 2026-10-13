// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File ▸ Import… and File ▸ Export… are one entry each: their dialog lists
//! the formats ("Supported formats" first for Import and Open), and the
//! picked file's extension says which import or export runs. Open takes
//! every format Import does: one that is not a document starts a new
//! document holding it.

use crate::common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::{window, TmpDir};
use xtrude_app::window::files::{format_of, formats_filter, EXPORT_FORMATS, IMPORT_FORMATS};
use xtrude_app::window::{MainWindow, ScriptedDialogs};

fn scripted(w: &mut MainWindow) -> Rc<RefCell<ScriptedDialogs>> {
    let d = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(d.clone());
    d
}

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

#[test]
fn test_the_file_menu_has_one_import_and_one_export() {
    let w = window();
    assert_eq!(
        w.actions.get("import").map(|a| a.text.as_str()),
        Some("Import…")
    );
    assert_eq!(
        w.actions.get("export").map(|a| a.text.as_str()),
        Some("Export…")
    );
    let per_format: Vec<&str> = w
        .actions
        .iter()
        .map(|a| a.id.as_str())
        .filter(|id| id.starts_with("import:") || id.starts_with("export:"))
        .collect();
    assert!(per_format.is_empty(), "{per_format:?}");
}

#[test]
fn test_the_filters_list_supported_formats_then_each_format() {
    let open = formats_filter(IMPORT_FORMATS, true);
    let lines: Vec<&str> = open.split(";;").collect();
    assert_eq!(
        lines[0],
        "Supported formats (*.xtrude *.igz *.step *.stp *.stl *.obj *.glb *.gltf *.dae *.dxf *.skp)"
    );
    assert_eq!(lines[1], "Xtrude CAD document (*.xtrude)");
    assert_eq!(lines.len(), IMPORT_FORMATS.len() + 2);
    assert_eq!(lines.last(), Some(&"All files (*)"));
    let export = formats_filter(EXPORT_FORMATS, false);
    assert!(export.starts_with("STL mesh, millimetres (*.stl);;3MF package, millimetres (*.3mf);;"));
    assert!(!export.contains("Supported formats"));
    assert_eq!(
        format_of(IMPORT_FORMATS, Path::new("a/part.STP")).map(|f| f.key),
        Some("step")
    );
    assert_eq!(format_of(IMPORT_FORMATS, Path::new("part.txt")), None);
}

#[test]
fn test_import_picks_the_format_from_the_extension() {
    let mut w = window();
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.push(Some(golden("box.obj")));
    d.borrow_mut().choices.push(Some(0)); // the OBJ's unit
    w.trigger("import");
    assert!(face_count(&w) > 0);
    assert!(w.viewport.history.can_undo());
    let d = d.borrow();
    assert_eq!(d.asked[0], ("open".to_string(), "Import".to_string()));
    assert!(d.filters[0].starts_with("Supported formats ("));
}

#[test]
fn test_import_of_an_unknown_format_says_so() {
    let dir = TmpDir::new();
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();
    let mut w = window();
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.push(Some(notes));
    w.trigger("import");
    let d = d.borrow();
    assert_eq!(d.asked.len(), 2, "{:?}", d.asked);
    assert_eq!(d.asked[1].0, "warn");
    assert!(d.asked[1].1.contains("notes.txt"), "{:?}", d.asked);
}

#[test]
fn test_open_of_another_format_starts_a_new_document_holding_it() {
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    let before = face_count(&w);
    w.viewport.scene.version += 1; // an edit, so Open asks first
    let d = scripted(&mut w);
    d.borrow_mut()
        .unsaved
        .push(xtrude_app::window::SaveChoice::Discard);
    d.borrow_mut().open_paths.push(Some(golden("box.obj")));
    d.borrow_mut().choices.push(Some(0)); // the OBJ's unit
    w.trigger("open");
    assert!(w.current_path.is_none());
    assert_eq!(w.import_name.as_deref(), Some("box.obj"));
    // the box once, not the document's box plus the OBJ's
    assert!(
        face_count(&w) > 0 && face_count(&w) <= before,
        "{} vs {before}",
        face_count(&w)
    );
    assert_eq!(
        d.borrow().asked[1],
        ("open".to_string(), "Open".to_string())
    );
}

#[test]
fn test_open_of_another_format_leaves_it_unsaved_whatever_the_case_of_its_extension() {
    let dir = TmpDir::new();
    let stl = dir.path().join("BOX.STL");
    std::fs::copy(golden("box.stl"), &stl).unwrap();
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    assert!(!w.is_dirty());
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.push(Some(stl));
    d.borrow_mut().stl.push(Some((2, 0))); // millimetres, no simplification
    w.trigger("open");
    assert_eq!(w.import_name.as_deref(), Some("BOX.STL"));
    assert!(w.current_path.is_none());
    assert!(face_count(&w) > 0);
    // a model that is in no file yet: New asks before it goes
    assert!(w.is_dirty());
    d.borrow_mut()
        .unsaved
        .push(xtrude_app::window::SaveChoice::Cancel);
    w.trigger("new");
    assert!(face_count(&w) > 0, "cancelled: the model stays");
    let asked = &d.borrow().asked;
    assert_eq!(asked.last().map(|(k, _)| k.as_str()), Some("unsaved"));
}

#[test]
fn test_export_writes_the_format_its_extension_names() {
    let dir = TmpDir::new();
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    let d = scripted(&mut w);
    let out = dir.path().join("box.obj");
    d.borrow_mut().save_paths.push(Some(out.clone()));
    w.trigger("export");
    assert!(std::fs::read_to_string(&out).unwrap().contains("\nf "));
    let d = d.borrow();
    assert_eq!(d.asked[0], ("save".to_string(), "Export".to_string()));
    assert_eq!(d.starts[0].file_name().unwrap(), "box.stl");
    assert!(d.filters[0].starts_with("STL mesh, millimetres (*.stl)"));
}

#[test]
fn test_export_without_an_extension_asks_which_format() {
    let dir = TmpDir::new();
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    let d = scripted(&mut w);
    let bare = dir.path().join("part");
    d.borrow_mut()
        .save_paths
        .extend([Some(bare.clone()), Some(bare.clone())]);
    d.borrow_mut().choices.extend([Some(0), None]);
    w.trigger("export");
    assert!(dir.path().join("part.stl").is_file());
    w.trigger("export"); // the question is cancelled: nothing is written
    let written: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(written.len(), 1);
    assert!(!bare.exists());
    let asked = &d.borrow().asked;
    assert_eq!(asked.iter().filter(|(k, _)| k == "choose").count(), 2);
}

/// What a model needs to be found again after an export and an Open:
/// some faces or edges.
fn has_geometry(w: &MainWindow) -> bool {
    face_count(w) > 0 || !w.viewport.scene.mesh().edges.is_empty()
}

#[test]
fn test_each_export_format_writes_its_file_and_mesh_ones_open_again() {
    let dir = TmpDir::new();
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    let d = scripted(&mut w);
    for (ext, magic) in [
        ("stl", &b""[..]),
        ("3mf", b"PK\x03\x04"),
        ("obj", b""),
        ("glb", b"glTF"),
        ("dae", b"<?xml"),
    ] {
        let out = dir.path().join(format!("box.{ext}"));
        d.borrow_mut().save_paths.push(Some(out.clone()));
        w.trigger("export");
        let bytes = std::fs::read(&out).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert!(!bytes.is_empty() && bytes.starts_with(magic), "{ext}");
        assert_eq!(
            w.status_message
                .as_ref()
                .map(|m| m.text.contains(&format!("box.{ext}"))),
            Some(true),
            "{ext}"
        );
    }
    assert!(
        d.borrow().asked.iter().all(|(k, _)| k == "save"),
        "{:?}",
        d.borrow().asked
    );
    // the formats Open reads, without a question, open as a new document
    for ext in ["glb", "dae"] {
        let mut w = window();
        let d = scripted(&mut w);
        d.borrow_mut()
            .open_paths
            .push(Some(dir.path().join(format!("box.{ext}"))));
        w.trigger("open");
        assert!(has_geometry(&w), "{ext}");
        assert_eq!(w.import_name, Some(format!("box.{ext}")));
        assert!(w.current_path.is_none());
        let asked = &d.borrow().asked;
        assert_eq!(asked.len(), 1, "{ext}: {asked:?}");
    }
}

#[test]
fn test_open_of_a_dxf_starts_a_new_document_with_its_linework() {
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    assert!(face_count(&w) > 0);
    let d = scripted(&mut w);
    let dxf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/dxf/lines_and_polylines.dxf");
    d.borrow_mut().open_paths.push(Some(dxf));
    d.borrow_mut().choices.push(Some(0)); // the DXF's unit
    w.trigger("open");
    assert_eq!(w.import_name.as_deref(), Some("lines_and_polylines.dxf"));
    assert!(w.current_path.is_none());
    assert!(!w.viewport.scene.doc.groups.is_empty());
    assert_eq!(face_count(&w), 0, "none of the box is left");
}

#[test]
fn test_open_of_an_unreadable_skp_reports_it_on_a_new_document() {
    let dir = TmpDir::new();
    let bad = dir.path().join("broken.skp");
    std::fs::write(&bad, b"not a .skp file at all").unwrap();
    let mut w = window();
    assert!(w.open_path(&golden("box.xtrude")));
    let d = scripted(&mut w);
    d.borrow_mut().open_paths.push(Some(bad));
    w.trigger("open");
    assert!(w.current_path.is_none());
    assert_eq!(face_count(&w), 0);
    let asked = &d.borrow().asked;
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[1].0, "warn");
}
