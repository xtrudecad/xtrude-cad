// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Tools named in the project's own words: each carries its label and id,
//! and the ids it had before are no longer tools.

use xtrude_app::window::make_tool;
use xtrude_model::solids::SolidOp;

/// `(id, label)` of a fresh tool for `key`.
fn named(key: &str) -> Option<(&'static str, &'static str)> {
    make_tool(key).map(|t| {
        let info = t.info();
        (info.key, info.name)
    })
}

#[test]
fn test_measure_is_the_measure_tool() {
    assert_eq!(named("measure"), Some(("measure", "Measure")));
    assert!(named("tape").is_none());
}

#[test]
fn test_merge_solids_is_the_merge_solids_tool() {
    assert_eq!(
        named("merge_solids"),
        Some(("merge_solids", "Merge Solids"))
    );
    assert!(named("outer_shell").is_none());
    let op = SolidOp::from_key("merge_solids");
    assert_eq!(op, Some(SolidOp::MergeSolids));
    assert_eq!(SolidOp::MergeSolids.result_name(), "Merged solids");
    assert_eq!(SolidOp::from_key("outer_shell"), None);
}

#[test]
fn test_sector_is_the_sector_tool() {
    assert_eq!(named("sector"), Some(("sector", "Sector")));
    assert!(named("pie").is_none());
}
