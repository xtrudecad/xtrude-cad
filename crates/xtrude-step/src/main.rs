// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `xtrude-step FILE [--chord-mm MM] [--angle-deg DEG]`: read a STEP file
//! with OpenCascade, tessellate it, and write the result on stdout in the
//! stream `xtrude_io::step::protocol` defines. Xtrude CAD runs it for
//! File › Import › STEP, so that a file OpenCascade fails on can only end
//! this process. Progress lines and errors go to stderr; the exit code
//! says how it went (`EXIT_*`).
//!
//! Built without the `occt` feature, it is a stub that exits with
//! `EXIT_NO_OCCT`.

#[cfg(feature = "occt")]
mod occt;

use std::process::ExitCode;

use xtrude_io::step::protocol::EXIT_USAGE;
use xtrude_io::step::Tessellation;

const USAGE: &str = "usage: xtrude-step FILE [--chord-mm MM] [--angle-deg DEG]";

/// The command line.
struct Args {
    file: String,
    // The stub only checks the command line; it never meshes.
    #[cfg_attr(not(feature = "occt"), allow(dead_code))]
    tessellation: Tessellation,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut file = None;
    let mut t = Tessellation::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut number = |what: &str| -> Result<f64, String> {
            it.next()
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| format!("{what} needs a positive number"))
        };
        match a.as_str() {
            "--chord-mm" => t.chord = number("--chord-mm")? / 1000.0,
            "--angle-deg" => t.angle_deg = number("--angle-deg")?,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return Err("one file only".into()),
        }
    }
    Ok(Args {
        file: file.ok_or("no file given")?,
        tessellation: t,
    })
}

fn exit(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("xtrude-step {} ({})", env!("CARGO_PKG_VERSION"), backend());
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(&args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("xtrude-step: {e}\n{USAGE}");
            return exit(EXIT_USAGE);
        }
    };
    run(&args)
}

#[cfg(feature = "occt")]
fn backend() -> &'static str {
    "OpenCascade 7.8.1"
}

#[cfg(not(feature = "occt"))]
fn backend() -> &'static str {
    "built without OpenCascade"
}

#[cfg(feature = "occt")]
fn run(args: &Args) -> ExitCode {
    use std::io::Write;
    use xtrude_io::step::protocol::{EXIT_FAILED, EXIT_OK};
    let Some(mut out) = result_output() else {
        eprintln!("cannot set up the output");
        // SAFETY: see below.
        unsafe { libc::_exit(EXIT_FAILED) }
    };
    let code = match occt::read(&args.file, &args.tessellation) {
        Ok(raw) => {
            let bytes = xtrude_io::step::protocol::encode(&raw);
            match out.write_all(&bytes).and_then(|_| out.flush()) {
                Ok(()) => EXIT_OK,
                Err(_) => EXIT_FAILED,
            }
        }
        Err(e) => {
            eprintln!("{e}");
            EXIT_FAILED
        }
    };
    let _ = std::io::stderr().flush();
    // Statically linked OpenCascade crashes in its static destructors at
    // exit (they run in the wrong order); everything is written, so leave
    // without running them.
    // SAFETY: `_exit` ends the process at once; no Rust state outlives it.
    unsafe { libc::_exit(code) }
}

/// Where the result goes: stdout, which carries it alone. On Unix the
/// real stdout is kept aside and whatever a library prints there goes to
/// stderr instead.
#[cfg(all(feature = "occt", unix))]
fn result_output() -> Option<std::fs::File> {
    use std::os::fd::FromRawFd;
    // SAFETY: plain descriptor calls on 1 and 2, which the process owns;
    // the returned file takes ownership of the duplicate.
    unsafe {
        let fd = libc::dup(1);
        if fd < 0 || libc::dup2(2, 1) < 0 {
            return None;
        }
        Some(std::fs::File::from_raw_fd(fd))
    }
}

#[cfg(all(feature = "occt", not(unix)))]
fn result_output() -> Option<std::io::Stdout> {
    Some(std::io::stdout())
}

#[cfg(not(feature = "occt"))]
fn run(args: &Args) -> ExitCode {
    eprintln!(
        "xtrude-step was built without OpenCascade (the `occt` feature): cannot read {}",
        args.file
    );
    exit(xtrude_io::step::protocol::EXIT_NO_OCCT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn tolerances_default_and_parse() {
        let a = args(&["part.step"]).unwrap();
        assert_eq!(a.file, "part.step");
        assert_eq!(a.tessellation, Tessellation::default());
        let a = args(&["--chord-mm", "0.05", "part.step", "--angle-deg", "15"]).unwrap();
        assert!((a.tessellation.chord - 0.05e-3).abs() < 1e-15);
        assert_eq!(a.tessellation.angle_deg, 15.0);
    }

    #[test]
    fn bad_command_lines_are_refused() {
        assert!(args(&[]).is_err());
        assert!(args(&["a.step", "b.step"]).is_err());
        assert!(args(&["a.step", "--chord-mm"]).is_err());
        assert!(args(&["a.step", "--chord-mm", "-1"]).is_err());
        assert!(args(&["a.step", "--angle-deg", "nan"]).is_err());
        assert!(args(&["a.step", "--fast"]).is_err());
    }
}
