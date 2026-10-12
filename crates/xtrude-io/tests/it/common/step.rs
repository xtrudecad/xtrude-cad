// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The STEP helper for the tests that read real files.

use std::path::{Path, PathBuf};
use std::process::Command;

use xtrude_io::step::helper::{helper_file_name, HELPER_ENV};
use xtrude_io::step::HelperBackend;

/// Set to make a missing helper fail the tests instead of skipping them
/// (the CI job that builds the helper sets it).
pub const REQUIRED_ENV: &str = "XTRUDE_STEP_REQUIRED";

/// A STEP fixture (`tests/data/step`).
pub fn fixture(name: &str) -> PathBuf {
    super::data("step").join(name)
}

fn has_occt(program: &Path) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .is_some_and(|v| v.contains("OpenCascade") && !v.contains("without"))
}

/// The helper built with OpenCascade: the one [`HELPER_ENV`] names, or a
/// release or debug build in the workspace's `target/`. `None` (the test
/// then skips) when there is none, unless [`REQUIRED_ENV`] is set.
pub fn helper() -> Option<HelperBackend> {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let candidates: Vec<PathBuf> = match std::env::var_os(HELPER_ENV) {
        Some(p) if !p.is_empty() => vec![PathBuf::from(p)],
        _ => ["release", "debug"]
            .iter()
            .map(|profile| target.join(profile).join(helper_file_name()))
            .collect(),
    };
    if let Some(found) = candidates.into_iter().find(|p| has_occt(p)) {
        return Some(HelperBackend::new(found));
    }
    assert!(
        std::env::var_os(REQUIRED_ENV).is_none(),
        "{REQUIRED_ENV} is set but no xtrude-step built with OpenCascade was found"
    );
    eprintln!("skipped: no xtrude-step built with OpenCascade (cargo build --release -p xtrude-step --features occt)");
    None
}
