// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `require` loads Lua modules from the extension's own folder only.

use xtrude_ext::{ErrorKind, Runtime};

use crate::common::TmpDir;

fn runtime_in(dir: &TmpDir) -> Runtime {
    let rt = Runtime::new().expect("Lua runtime");
    rt.set_module_root(&dir.0).expect("module root");
    rt
}

#[test]
fn require_loads_a_module_once() {
    let dir = TmpDir::new();
    dir.write(
        "util/geom.lua",
        "count = (count or 0) + 1\nreturn { twice = function(x) return 2 * x end }\n",
    );
    let rt = runtime_in(&dir);
    let (a, n): (i64, i64) = rt
        .run_chunk(
            "local g = require 'util.geom'\nlocal h = require 'util.geom'\nassert(g == h)\nreturn g.twice(21), count",
            "init.lua",
        )
        .expect("require works");
    assert_eq!((a, n), (42, 1));
}

#[test]
fn require_stays_in_the_folder() {
    let dir = TmpDir::new();
    dir.write("ext/init.lua", "");
    dir.write("secret.lua", "return 'leaked'");
    let rt = Runtime::new().expect("Lua runtime");
    rt.set_module_root(dir.0.join("ext")).expect("module root");
    for name in ["..secret", "../secret", "/etc/passwd", "a..b", ""] {
        let err = rt
            .run_chunk::<()>(&format!("require {name:?}"), "init.lua")
            .expect_err("an invalid module name must fail");
        assert_eq!(err.kind, ErrorKind::Runtime);
        assert!(
            err.message.contains("invalid module name"),
            "{}",
            err.message
        );
    }
}

#[test]
fn missing_module_is_an_error() {
    let dir = TmpDir::new();
    let rt = runtime_in(&dir);
    let err = rt
        .run_chunk::<()>("require 'nothing'", "init.lua")
        .expect_err("a missing module must fail");
    assert!(
        err.message.contains("module 'nothing' not found"),
        "{}",
        err.message
    );
}

#[test]
fn module_syntax_error_names_the_module_file() {
    let dir = TmpDir::new();
    dir.write("broken.lua", "return {\n");
    let rt = runtime_in(&dir);
    let err = rt
        .run_chunk::<()>("require 'broken'", "init.lua")
        .expect_err("a broken module must fail");
    assert_eq!(err.kind, ErrorKind::Syntax);
    assert!(err.message.contains("broken.lua:"), "{}", err.message);
}

#[test]
fn a_module_requiring_itself_is_an_error() {
    let dir = TmpDir::new();
    dir.write("loop.lua", "return require 'loop'");
    let rt = runtime_in(&dir);
    let err = rt
        .run_chunk::<()>("require 'loop'", "init.lua")
        .expect_err("a require cycle must fail");
    assert!(err.message.contains("requires itself"), "{}", err.message);
}
