// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Per-document data (`doc:data`, `doc:set_data`) and the exports, which
//! write only where the user chose.

use std::path::PathBuf;

use mlua::Value;
use serde_json::json;
use xtrude_ext::ScriptCommand;
use xtrude_io::document;
use xtrude_model::EntityRef;

use crate::common::{self, Fixture, TmpDir};

/// Runs `source` as `label` of the extension `ext`; `ask_save_path`
/// answers each question with the next of `paths`, and records it.
fn run_with_dialog(
    fx: &mut Fixture,
    ext: &str,
    source: &str,
    paths: Vec<Option<PathBuf>>,
) -> (bool, Vec<(String, String)>) {
    let func = fx.rt.load(source, "test.lua").expect("compiles");
    let mut answers = paths.into_iter();
    let mut asked = Vec::new();
    let ok = {
        let cmd = ScriptCommand::new(&fx.rt, ext, "Script", func, Value::Nil).with_save_dialog(
            |title: &str, filter: &str| {
                asked.push((title.to_string(), filter.to_string()));
                answers.next().flatten()
            },
        );
        let mut cmd = cmd;
        fx.history.execute(&mut fx.scene, &mut cmd)
    };
    (ok, asked)
}

const SQUARE: &str = "doc:add_face({ xtrude.vec(0, 0), xtrude.vec(10, 0), \
                      xtrude.vec(10, 10), xtrude.vec(0, 10) })";

#[test]
fn data_round_trips_through_a_document_file() {
    let mut fx = common::fixture();
    let dir = TmpDir::new();
    assert!(fx.run(
        "Store",
        "local doc = ...\nassert(doc:data() == nil)\n\
         doc:set_data({ teeth = 20, name = 'spur', sizes = { 1.5, 2 }, on = true, nested = { a = 'b' } })",
    ));
    assert_eq!(
        fx.scene.doc.plugin_data.get("test"),
        Some(
            &json!({ "name": "spur", "nested": { "a": "b" }, "on": true, "sizes": [1.5, 2], "teeth": 20 })
        )
    );
    let path = dir.0.join("data.xtr");
    document::save_scene(&fx.scene, &path).expect("saves");
    let mut back = common::fixture();
    document::load_into(&mut back.scene, &path, None).expect("loads");
    assert!(back.run(
        "Read",
        "local doc = ...\nlocal d = doc:data()\n\
         assert(d.teeth == 20 and math.type(d.teeth) == 'integer')\n\
         assert(d.name == 'spur' and d.on == true and d.nested.a == 'b')\n\
         assert(#d.sizes == 2 and d.sizes[1] == 1.5)",
    ));
    assert_eq!(back.history.last_error, None);
}

#[test]
fn other_keys_are_untouched_and_unseen() {
    let mut fx = common::fixture();
    let ingetrazo = json!({ "version": 3, "panels": [1, 2, { "x": null }] });
    fx.scene
        .doc
        .plugin_data
        .insert("ingetrazo_stairs".into(), ingetrazo.clone());
    assert!(fx.run(
        "Write",
        "local doc = ...\nassert(doc:data() == nil)\ndoc:set_data({ n = 1 })",
    ));
    assert_eq!(
        fx.scene.doc.plugin_data.get("ingetrazo_stairs"),
        Some(&ingetrazo)
    );
    assert_eq!(fx.scene.doc.plugin_data.len(), 2);
    // nil removes the extension's own key only
    assert!(fx.run("Clear", "local doc = ...\ndoc:set_data(nil)"));
    assert_eq!(fx.scene.doc.plugin_data.len(), 1);
    assert_eq!(
        fx.scene.doc.plugin_data.get("ingetrazo_stairs"),
        Some(&ingetrazo)
    );
}

#[test]
fn undo_restores_the_previous_data() {
    let mut fx = common::fixture();
    assert!(fx.run("First", "local doc = ...\ndoc:set_data({ n = 1 })"));
    assert!(fx.run("Second", "local doc = ...\ndoc:set_data({ n = 2 })"));
    assert_eq!(
        fx.scene.doc.plugin_data.get("test"),
        Some(&json!({ "n": 2 }))
    );
    assert!(fx.history.undo(&mut fx.scene));
    assert_eq!(
        fx.scene.doc.plugin_data.get("test"),
        Some(&json!({ "n": 1 }))
    );
    assert!(fx.history.undo(&mut fx.scene));
    assert_eq!(fx.scene.doc.plugin_data.get("test"), None);
}

#[test]
fn a_one_off_script_keeps_no_data() {
    let mut fx = common::fixture();
    let (ok, _) = run_with_dialog(
        &mut fx,
        "",
        "local doc = ...\ndoc:set_data({ n = 1 })",
        vec![],
    );
    assert!(!ok);
    let err = fx.history.last_error.clone().unwrap_or_default();
    assert!(err.contains("only an extension keeps data"), "{err}");
    assert!(fx.scene.doc.plugin_data.is_empty());
}

#[test]
fn exports_write_where_the_user_chose() {
    let mut fx = common::fixture();
    let dir = TmpDir::new();
    let stl = dir.0.join("part.stl");
    let obj = dir.0.join("part.obj");
    let source = format!(
        "local doc = ...\n{SQUARE}\n\
         local p = xtrude.ask_save_path('Export the part', 'STL mesh (*.stl)')\n\
         doc:export_stl(p)\n\
         local q = xtrude.ask_save_path('Export OBJ', 'Wavefront OBJ (*.obj)')\n\
         doc:export_obj(q)\n\
         assert(xtrude.ask_save_path('Again') == nil)"
    );
    let (ok, asked) = run_with_dialog(
        &mut fx,
        "test",
        &source,
        vec![Some(stl.clone()), Some(obj.clone()), None],
    );
    assert!(ok, "{:?}", fx.history.last_error);
    assert_eq!(
        asked,
        [
            ("Export the part".into(), "STL mesh (*.stl)".into()),
            ("Export OBJ".into(), "Wavefront OBJ (*.obj)".into()),
            ("Again".into(), String::new()),
        ]
    );
    let bytes = std::fs::read(&stl).expect("the STL");
    // header, count, two triangles of 50 bytes
    assert_eq!(bytes.len(), 84 + 2 * 50);
    let text = std::fs::read_to_string(&obj).expect("the OBJ");
    assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 4);
}

#[test]
fn a_raw_path_is_refused() {
    let mut fx = common::fixture();
    let dir = TmpDir::new();
    let target = dir.0.join("sneaky.stl");
    let source = format!(
        "local doc = ...\n{SQUARE}\ndoc:export_stl({:?})",
        target.to_string_lossy()
    );
    let (ok, _) = run_with_dialog(&mut fx, "test", &source, vec![]);
    assert!(!ok);
    assert!(!target.exists());
    let err = fx.history.last_error.clone().unwrap_or_default();
    assert!(err.contains("xtrude.ask_save_path"), "{err}");
    // a path chosen in an earlier run is no longer allowed
    let (ok, _) = run_with_dialog(
        &mut fx,
        "test",
        "xtrude.ask_save_path('Save')",
        vec![Some(target.clone())],
    );
    assert!(ok);
    let source = format!(
        "local doc = ...\ndoc:export_obj({:?})",
        target.to_string_lossy()
    );
    let (ok, _) = run_with_dialog(&mut fx, "test", &source, vec![]);
    assert!(!ok);
    assert!(!target.exists());
    // without a dialog, the question is cancelled
    assert!(fx.run("Ask", "assert(xtrude.ask_save_path('Save') == nil)"));
}

#[test]
fn selection_only_exports_the_selected_groups() {
    let mut fx = common::fixture();
    let dir = TmpDir::new();
    let stl = dir.0.join("one.stl");
    assert!(fx.run(
        "Build",
        "local doc = ...\n\
         local a = doc:add_face({ xtrude.vec(0, 0), xtrude.vec(10, 0), xtrude.vec(10, 10), xtrude.vec(0, 10) })\n\
         doc:push_pull(a, 5)\n\
         local g = doc:make_group(doc:faces(), 'box')\n\
         doc:add_face({ xtrude.vec(20, 0), xtrude.vec(30, 0), xtrude.vec(30, 10) })\n\
         doc:select({ g })",
    ));
    let before = fx.scene.doc.groups.len();
    let (ok, _) = run_with_dialog(
        &mut fx,
        "test",
        "local doc = ...\ndoc:export_stl(xtrude.ask_save_path('STL'), { selection_only = true })",
        vec![Some(stl.clone())],
    );
    assert!(ok, "{:?}", fx.history.last_error);
    let bytes = std::fs::read(&stl).expect("the STL");
    // the box only: six quads, twelve triangles
    assert_eq!(bytes.len(), 84 + 12 * 50);
    assert_eq!(
        fx.scene.doc.groups.len(),
        before,
        "the document is whole again"
    );
    assert_eq!(fx.scene.loose_mesh().face_ids().len(), 1);
    assert!(matches!(
        fx.scene.selection.iter().next(),
        Some(EntityRef::Group(_))
    ));
}
