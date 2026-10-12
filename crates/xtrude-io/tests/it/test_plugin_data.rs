// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! .igz tests: any JSON value travels. (Plugin data is JSON values by
//! type, so nothing unserialisable can be stored.)

use crate::common;
use common::*;
use serde_json::json;
use xtrude_io::igz;
use xtrude_model::scene::Scene;

fn job() -> serde_json::Value {
    json!({"units": "mm", "stock": {"x": 600.0, "y": 400.0, "z": 18.0},
           "items": [{"kind": "profile", "texture": "not an image", "tabs": [1, 2, 3], "nested": {"a": null}}]})
}

#[test]
fn test_plugin_data_survives_save_and_reopen() {
    let mut s = Scene::new();
    s.doc.plugin_data.insert("myext".into(), job());
    let back = roundtrip(&s);
    assert_eq!(
        serde_json::Value::Object(back.doc.plugin_data),
        json!({"myext": job()})
    );
}

#[test]
fn test_a_document_without_plugin_data_stays_terse() {
    let tmp = TmpDir::new();
    let p = tmp.join("m.igz");
    igz::save_scene(&Scene::new(), &p).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert!(doc["scene"].get("plugin_data").is_none());
}

#[test]
fn test_opening_a_document_does_not_inherit_the_previous_ones() {
    let tmp = TmpDir::new();
    let p = tmp.join("plain.igz");
    igz::save_scene(&Scene::new(), &p).unwrap();
    let mut s = Scene::new();
    s.doc.plugin_data.insert("myext".into(), job());
    igz::load_into(&mut s, &p, None).unwrap();
    assert!(s.doc.plugin_data.is_empty());
}

#[test]
fn test_data_of_a_plugin_not_installed_is_kept() {
    let mut s = Scene::new();
    s.doc
        .plugin_data
        .insert("someone_elses".into(), json!({"x": 1}));
    let again = roundtrip(&roundtrip(&s));
    assert_eq!(
        serde_json::Value::Object(again.doc.plugin_data),
        json!({"someone_elses": {"x": 1}})
    );
}

#[test]
fn test_an_unserialisable_entry_costs_that_extension_only() {
    let mut s = Scene::new();
    s.doc.plugin_data.insert("good".into(), json!({"x": 1}));
    s.doc.plugin_data.insert("alist".into(), json!([1, 2]));
    let back = roundtrip(&s);
    assert_eq!(
        serde_json::Value::Object(back.doc.plugin_data),
        json!({"good": {"x": 1}, "alist": [1, 2]})
    );
}

#[test]
fn test_plugin_data_is_read_back_as_written_and_a_non_object_is_ignored() {
    let tmp = TmpDir::new();
    let p = tmp.join("m.igz");
    igz::save_scene(&Scene::new(), &p).unwrap();
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    doc["scene"]["plugin_data"] = json!({"ok": {"a": 1}, "n": 7});
    std::fs::write(&p, doc.to_string()).unwrap();
    let mut back = Scene::new();
    igz::load_into(&mut back, &p, None).unwrap();
    assert_eq!(
        serde_json::Value::Object(back.doc.plugin_data.clone()),
        json!({"ok": {"a": 1}, "n": 7})
    );
    doc["scene"]["plugin_data"] = json!("garbage");
    std::fs::write(&p, doc.to_string()).unwrap();
    igz::load_into(&mut back, &p, None).unwrap();
    assert!(back.doc.plugin_data.is_empty());
}
