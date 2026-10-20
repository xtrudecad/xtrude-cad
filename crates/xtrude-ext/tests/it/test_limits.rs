// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The time and memory limits end a run with an error.

use xtrude_ext::ErrorKind;

use crate::common;

#[test]
fn endless_loop_hits_the_time_limit() {
    let rt = common::tight_runtime();
    let err = rt
        .run_chunk::<()>("while true do end", "loop.lua")
        .expect_err("the loop must be stopped");
    assert_eq!(err.kind, ErrorKind::TimeLimit);
    assert!(err.message.contains("time limit"), "{}", err.message);
}

#[test]
fn pcall_cannot_swallow_the_time_limit() {
    let rt = common::tight_runtime();
    let err = rt
        .run_chunk::<()>(
            "while true do pcall(function() while true do end end) end",
            "loop.lua",
        )
        .expect_err("the loop must be stopped");
    assert_eq!(err.kind, ErrorKind::TimeLimit);
    let err = rt
        .run_chunk::<()>(
            "while true do xpcall(function() while true do end end, function() end) end",
            "loop.lua",
        )
        .expect_err("the loop must be stopped");
    assert_eq!(err.kind, ErrorKind::TimeLimit);
}

#[test]
fn the_time_limit_is_per_run() {
    let rt = common::tight_runtime();
    assert!(rt.run_chunk::<()>("while true do end", "loop.lua").is_err());
    let n: i64 = rt
        .run_chunk(
            "local n = 0 for i = 1, 1000 do n = n + i end return n",
            "sum.lua",
        )
        .expect("a short run after an aborted one");
    assert_eq!(n, 500_500);
}

#[test]
fn huge_allocation_hits_the_memory_limit() {
    let rt = common::memory_runtime();
    let err = rt
        .run_chunk::<()>("local s = string.rep('x', 1 << 30)", "big.lua")
        .expect_err("the allocation must fail");
    assert_eq!(err.kind, ErrorKind::Memory);
    assert!(err.message.contains("16 MiB"), "{}", err.message);
}

#[test]
fn growing_table_hits_the_memory_limit() {
    let rt = common::memory_runtime();
    let err = rt
        .run_chunk::<()>("local t = {} while true do t[#t + 1] = {} end", "grow.lua")
        .expect_err("the table must stop growing");
    assert_eq!(err.kind, ErrorKind::Memory);
}
