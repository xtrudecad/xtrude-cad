// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print the canonical JSON dump of a parsed `.skp` (the format of the
//! fixtures' `*.expected.json`); with a second argument, compare it with
//! that file and print the first difference.
//!
//!     cargo run -p openskp --example openskp_dump -- model.skp [model.expected.json]

#[path = "../tests/it/common/dump.rs"]
mod dump;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: openskp_dump <file.skp> [expected.json]");
        return ExitCode::from(2);
    };
    let model = match openskp::SkpFile::open(path).and_then(|f| f.parse()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let got = dump::dump(&model);
    let Some(expected) = args.get(1) else {
        println!("{got}");
        return ExitCode::SUCCESS;
    };
    let want: serde_json::Value = match std::fs::read_to_string(expected)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{expected}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match dump::first_difference("$", &want, &got, 1e-9) {
        None => {
            println!("{path}: identical");
            ExitCode::SUCCESS
        }
        Some(d) => {
            println!("{path}: {d}");
            ExitCode::FAILURE
        }
    }
}
