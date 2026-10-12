// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Extensions found in a folder: manifests, the skip rules, one-off
//! scripts.

use xtrude_ext::{discover, ErrorKind, Extension, Script};
use xtrude_model::{History, Scene};

use crate::common::TmpDir;

/// A manifest for the folder `id` with one command per `(id, label)`.
fn manifest(id: &str, commands: &[(&str, &str)]) -> String {
    let cmds: Vec<String> = commands
        .iter()
        .map(|(c, label)| {
            format!(
                "{{ id = '{c}', label = '{label}', run = function(doc, p) \
                 doc:add_face({{ xtrude.vec(0, 0), xtrude.vec(p.size or 10, 0), \
                 xtrude.vec(p.size or 10, p.size or 10) }}) end }}"
            )
        })
        .collect();
    format!(
        "return {{ id = '{id}', name = 'Ext {id}', version = '1.0', api = 1, commands = {{ {} }} }}",
        cmds.join(", ")
    )
}

#[test]
fn extensions_load_in_folder_order() {
    let dir = TmpDir::new();
    dir.write("zeta/init.lua", &manifest("zeta", &[("z1", "Zed")]));
    dir.write(
        "alpha/init.lua",
        &manifest("alpha", &[("a1", "First…"), ("a2", "Second")]),
    );
    dir.write("notes.txt", "not an extension");
    dir.write(".hidden/init.lua", "error('never loaded')");
    let found: Vec<Extension> = discover(&dir.0)
        .into_iter()
        .map(|r| r.expect("loads"))
        .collect();
    let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "zeta"]);
    let alpha = &found[0];
    assert_eq!(alpha.name, "Ext alpha");
    assert_eq!(alpha.version, "1.0");
    assert_eq!(alpha.api, 1);
    let cmds: Vec<(&str, &str)> = alpha
        .commands
        .iter()
        .map(|c| (c.id.as_str(), c.label.as_str()))
        .collect();
    assert_eq!(cmds, [("a1", "First…"), ("a2", "Second")]);
}

#[test]
fn a_missing_folder_holds_no_extension() {
    let dir = TmpDir::new();
    assert!(discover(&dir.0.join("absent")).is_empty());
}

#[test]
fn broken_extensions_are_skipped_with_their_reason() {
    let dir = TmpDir::new();
    dir.write("broken/init.lua", "return {");
    dir.write("good/init.lua", &manifest("good", &[("c", "C")]));
    dir.write("other/init.lua", &manifest("renamed", &[("c", "C")]));
    dir.write(
        "future/init.lua",
        "return { id = 'future', name = 'Future', api = 2 }",
    );
    dir.write("empty/readme.txt", "no init.lua here");
    dir.write("noapi/init.lua", "return { id = 'noapi', name = 'No API' }");
    dir.write(
        "dup/init.lua",
        "return { id = 'dup', name = 'Dup', api = 1, commands = { \
         { id = 'c', label = 'A', run = function() end }, \
         { id = 'c', label = 'B', run = function() end } } }",
    );
    let found = discover(&dir.0);
    let mut loaded = Vec::new();
    let mut failed = Vec::new();
    for r in found {
        match r {
            Ok(ext) => loaded.push(ext.id),
            Err((path, e)) => {
                let folder = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                failed.push((folder, e));
            }
        }
    }
    assert_eq!(loaded, ["good"]);
    let folders: Vec<&str> = failed.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(
        folders,
        ["broken", "dup", "empty", "future", "noapi", "other"]
    );
    let reason = |folder: &str| {
        failed
            .iter()
            .find(|(f, _)| f == folder)
            .map(|(_, e)| e.clone())
            .expect("a reason")
    };
    let broken = reason("broken");
    assert_eq!(broken.kind, ErrorKind::Syntax);
    assert!(broken.message.contains("broken/init.lua:1:"), "{broken:?}");
    assert!(reason("other").message.contains("does not match"));
    assert!(reason("future").message.contains("API version 2"));
    assert!(reason("empty").message.contains("init.lua"));
    assert!(reason("noapi").message.contains("'api'"));
    assert!(reason("dup").message.contains("Two commands"));
    assert_eq!(reason("other").kind, ErrorKind::Manifest);
}

#[test]
fn a_command_runs_with_its_default_params() {
    xtrude_model::settings::use_memory_store();
    let dir = TmpDir::new();
    dir.write(
        "tri/init.lua",
        "return { id = 'tri', name = 'Triangle', api = 1, commands = { \
         { id = 'make', label = 'Triangle…', \
           params = { { key = 'size', label = 'Size', type = 'length', default = 30 } }, \
           run = function(doc, p) doc:add_face({ xtrude.vec(0, 0), xtrude.vec(p.size, 0), \
             xtrude.vec(0, p.size) }) end } } }",
    );
    let ext = Extension::load(&dir.0.join("tri")).expect("loads");
    let mut scene = Scene::new();
    let mut history = History::new();
    history.error_log = Some(dir.0.join("errors.log"));
    let defaults = ext.command("make").expect("the command").defaults();
    assert_eq!(defaults, [xtrude_ext::ParamValue::Length(30.0)]);
    let mut cmd = ext
        .script_command("make", &defaults)
        .expect("the command exists")
        .expect("its params");
    assert!(history.execute(&mut scene, &mut cmd));
    assert_eq!(history.undo_name(), Some("Triangle"));
    let (lo, hi) = scene.compute_bounds().expect("a face");
    // 30 mm, in metres
    assert!(((hi - lo).x - 0.03).abs() < 1e-9, "{lo} {hi}");
    assert!(ext.script_command("absent", &[]).is_none());
}

#[test]
fn a_script_requires_modules_next_to_it() {
    xtrude_model::settings::use_memory_store();
    let dir = TmpDir::new();
    dir.write("lib/size.lua", "return 20");
    let path = dir.write(
        "square.lua",
        "local doc = ...\nlocal s = require 'lib.size'\n\
         doc:add_face({ xtrude.vec(0, 0), xtrude.vec(s, 0), xtrude.vec(s, s), xtrude.vec(0, s) })",
    );
    let script = Script::load(&path).expect("compiles");
    assert_eq!(script.name, "square.lua");
    let mut scene = Scene::new();
    let mut history = History::new();
    history.error_log = Some(dir.0.join("errors.log"));
    assert!(history.execute(&mut scene, &mut script.command("Square")));
    let (lo, hi) = scene.compute_bounds().expect("a face");
    assert!(((hi - lo).y - 0.02).abs() < 1e-9, "{lo} {hi}");
}

#[test]
fn an_unreadable_script_is_an_error() {
    let dir = TmpDir::new();
    let err = Script::load(&dir.0.join("absent.lua"))
        .err()
        .expect("no such file");
    assert!(err.message.contains("absent.lua"), "{err:?}");
    dir.write("bad.lua", "x = = 1");
    let err = Script::load(&dir.0.join("bad.lua"))
        .err()
        .expect("bad syntax");
    assert_eq!(err.kind, ErrorKind::Syntax);
    assert!(err.message.contains("bad.lua:1:"), "{err:?}");
}
