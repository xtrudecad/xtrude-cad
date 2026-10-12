// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The community files: where people ask, report and reach the
//! maintainer. Each private matter has its own address, and the public
//! files send questions and security reports to the same places.

use std::path::Path;

const REPO: &str = "https://github.com/xtrudecad/xtrude-cad";

fn read(path: &str) -> String {
    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn security_reports_go_to_a_private_channel() {
    let text = read("SECURITY.md");
    assert!(text.contains("security@xtrudecad.org"));
    assert!(text.contains(&format!("{REPO}/security/advisories/new")));
}

#[test]
fn conduct_reports_go_to_their_own_address() {
    let text = read("CODE_OF_CONDUCT.md");
    assert!(text.contains("**conduct@xtrudecad.org**"));
    assert!(text.contains("version 2.1"));
}

#[test]
fn the_readme_lists_every_channel() {
    let text = read("README.md");
    for channel in [
        format!("{REPO}/issues"),
        format!("{REPO}/discussions"),
        "SECURITY.md".to_owned(),
        "CODE_OF_CONDUCT.md".to_owned(),
        "conduct@xtrudecad.org".to_owned(),
        "hello@xtrudecad.org".to_owned(),
    ] {
        assert!(text.contains(&channel), "README.md misses {channel}");
    }
}

#[test]
fn issues_are_forms_and_questions_go_to_discussions() {
    let config = read(".github/ISSUE_TEMPLATE/config.yml");
    assert!(config.contains("blank_issues_enabled: false"));
    assert!(config.contains(&format!("url: {REPO}/discussions\n")));
    // GitHub lists its own private-report entry while private
    // vulnerability reporting is on; a link of ours would repeat it.
    assert!(!config.contains("/security/advisories/new"));
    for form in ["bug_report.yml", "feature_request.yml"] {
        let text = read(&format!(".github/ISSUE_TEMPLATE/{form}"));
        assert!(text.contains("\nbody:\n"), "{form} is not an issue form");
    }
}
