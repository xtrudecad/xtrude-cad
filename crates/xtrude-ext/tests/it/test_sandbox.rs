// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A script sees only the safe libraries: nothing reaches files, the
//! process or the interpreter's internals.

use xtrude_ext::Runtime;

fn eval<R: mlua::FromLuaMulti>(src: &str) -> R {
    Runtime::new()
        .expect("Lua runtime")
        .run_chunk(src, "test.lua")
        .expect("chunk runs")
}

#[test]
fn unsafe_libraries_are_absent() {
    for name in [
        "io",
        "os",
        "package",
        "debug",
        "coroutine",
        "require",
        "dofile",
        "loadfile",
    ] {
        let kind: String = eval(&format!("return type({name})"));
        assert_eq!(kind, "nil", "{name} must not exist");
    }
}

#[test]
fn os_execute_and_package_loadlib_cannot_be_reached() {
    let reached: bool = eval("return pcall(function() return os.execute('true') end)");
    assert!(!reached);
    let reached: bool =
        eval("return pcall(function() return package.loadlib('libc.so.6', 'abort') end)");
    assert!(!reached);
}

#[test]
fn safe_libraries_are_present() {
    let out: String =
        eval("return table.concat({ string.upper('a'), math.floor(2.5), utf8.char(0x41) }, ',')");
    assert_eq!(out, "A,2,A");
}

#[test]
fn load_takes_source_text() {
    let n: i64 = eval("return load('return 1 + 2')()");
    assert_eq!(n, 3);
    // An explicit nil environment still counts as an environment: the
    // loaded chunk sees no globals at all.
    let reached: bool = eval("return pcall(load('return math', 'x', 'bt', nil))");
    assert!(!reached);
}

#[test]
fn load_refuses_bytecode() {
    let (f, msg): (mlua::Value, String) =
        eval("return load(string.dump(function() return 1 end), 'dumped', 'b')");
    assert!(f.is_nil());
    assert!(msg.contains("binary"), "{msg}");
}

#[test]
fn run_chunk_refuses_bytecode() {
    let rt = Runtime::new().expect("Lua runtime");
    let bytecode: mlua::LuaString = rt
        .run_chunk("return string.dump(function() return 1 end)", "dump.lua")
        .expect("dump");
    let source = String::from_utf8_lossy(&bytecode.as_bytes()).into_owned();
    // The text keeps the bytecode signature ("\x1bLua"), which is
    // what marks a chunk as binary.
    let err = rt
        .run_chunk::<mlua::Value>(&source, "bytecode.lua")
        .expect_err("bytecode must be refused");
    assert!(err.message.contains("binary"), "{}", err.message);
}
