// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Value-box tests: the saving half is in xtrude-io's
//! test_non_finite_values.

use crate::common;

use common::{close, MetreDocument};
use xtrude_app::value_box::{parse_length_field, parse_value_buffer, Typed};

#[test]
fn test_the_value_box_refuses_a_number_too_long_for_a_float() {
    let nines = "9".repeat(400);
    for typed in [
        nines.clone(),
        format!("{nines};1"),
        format!("1;{nines}"),
        format!("{nines}:12"),
        format!("1:{nines}"),
    ] {
        assert_eq!(parse_value_buffer(&typed, false), None);
    }
}

#[test]
fn test_ordinary_numbers_still_parse() {
    let _m = MetreDocument::new();
    let Some(Typed::Value(v)) = parse_value_buffer("2.5", false) else {
        panic!()
    };
    assert!(close(v, 2.5));
    assert!(close(
        parse_length_field("3/4\"").unwrap_or(0.0),
        0.75 * 0.0254
    ));
    assert_eq!(parse_value_buffer("1e5", false), None);
}
