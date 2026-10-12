// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Errors carry the chunk, the line and the traceback, and fail a command.

use xtrude_ext::{ErrorKind, ExtError, Runtime};
use xtrude_model::CmdError;

fn run(src: &str, name: &str) -> ExtError {
    Runtime::new()
        .expect("Lua runtime")
        .run_chunk::<()>(src, name)
        .expect_err("the chunk must fail")
}

#[test]
fn syntax_error_names_file_and_line() {
    let err = run("local a = 1\nlocal b = = 2\n", "gears/init.lua");
    assert_eq!(err.kind, ErrorKind::Syntax);
    assert!(err.message.contains("gears/init.lua:2:"), "{}", err.message);
}

#[test]
fn runtime_error_names_file_line_and_traceback() {
    let err = run(
        "local function inner()\n  error('boom')\nend\ninner()\n",
        "box.lua",
    );
    assert_eq!(err.kind, ErrorKind::Runtime);
    assert!(err.message.contains("box.lua:2: boom"), "{}", err.message);
    let tb = err.traceback.as_deref().expect("a traceback");
    assert!(tb.contains("box.lua:2"), "{tb}");
    assert!(tb.contains("box.lua:4"), "{tb}");
}

#[test]
fn ext_error_fails_a_command_with_its_report() {
    let err = run("error('nope')", "x.lua");
    let report = err.report();
    assert!(report.contains("x.lua:1: nope"), "{report}");
    assert!(report.contains("stack traceback:"), "{report}");
    let cmd: CmdError = err.into();
    assert_eq!(cmd.to_string(), report);
}

#[test]
fn ext_error_is_a_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(run("error('nope')", "x.lua"));
    assert!(err.to_string().contains("nope"));
}
