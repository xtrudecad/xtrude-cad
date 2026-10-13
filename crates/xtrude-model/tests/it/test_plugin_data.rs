// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Plugin data: the document half (the document round trips live in xtrude-io).

use serde_json::json;
use xtrude_model::commands::SetPluginDataCommand;
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

#[test]
fn test_file_new_clears_plugin_data() {
    let mut s = Scene::new();
    s.doc.plugin_data.insert("myext".into(), json!({"x": 1}));
    let v = s.version;
    s.clear();
    assert!(s.doc.plugin_data.is_empty());
    assert!(s.version > v);
}

#[test]
fn test_set_plugin_data_is_undoable_and_isolated_from_the_live_dict() {
    let mut s = Scene::new();
    let mut h = History::new();
    let mut job = json!({"n": 1});
    h.execute(
        &mut s,
        &mut SetPluginDataCommand {
            key: "myext".into(),
            value: Some(job.clone()),
        },
    );
    job["n"] = json!(99); // the plugin keeps editing its own value
    assert_eq!(s.doc.plugin_data.get("myext"), Some(&json!({"n": 1})));
    h.execute(
        &mut s,
        &mut SetPluginDataCommand {
            key: "myext".into(),
            value: Some(json!({"n": 2})),
        },
    );
    assert_eq!(s.doc.plugin_data["myext"], json!({"n": 2}));
    s.doc.plugin_data.insert("myext".into(), json!({"n": 50})); // live edit
    h.undo(&mut s);
    assert_eq!(s.doc.plugin_data["myext"], json!({"n": 1}));
    h.undo(&mut s);
    assert!(!s.doc.plugin_data.contains_key("myext"));
    h.redo(&mut s);
    h.redo(&mut s);
    assert_eq!(s.doc.plugin_data["myext"], json!({"n": 2}));
}

#[test]
fn test_set_plugin_data_none_removes_the_entry() {
    let mut s = Scene::new();
    s.doc.plugin_data.insert("myext".into(), json!({"n": 1}));
    let mut h = History::new();
    h.execute(
        &mut s,
        &mut SetPluginDataCommand {
            key: "myext".into(),
            value: None,
        },
    );
    assert!(!s.doc.plugin_data.contains_key("myext"));
    h.undo(&mut s);
    assert_eq!(s.doc.plugin_data.get("myext"), Some(&json!({"n": 1})));
}

#[test]
fn test_plugin_data_edits_mark_the_document_modified() {
    let mut s = Scene::new();
    let mut h = History::new();
    let v0 = s.version;
    h.execute(
        &mut s,
        &mut SetPluginDataCommand {
            key: "x".into(),
            value: Some(json!({"a": 1})),
        },
    );
    assert!(s.version > v0);
    let v1 = s.version;
    h.undo(&mut s);
    assert!(s.version > v1);
}
