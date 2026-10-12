// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Typed lengths in inches and feet next to metres.

use crate::common;

use common::MetreDocument;
use xtrude_app::input::KeyEvent;
use xtrude_app::value_box::{parse_length_field, parse_value_buffer, Typed, ValueBox};
use xtrude_model::units::format_length;

const IN: f64 = 0.0254;
const FT: f64 = 0.3048;

fn parse(s: &str) -> Option<Typed> {
    parse_value_buffer(s, false)
}

fn one(s: &str) -> f64 {
    match parse(s) {
        Some(Typed::Value(v)) => v,
        other => panic!("{s:?} -> {other:?}"),
    }
}

fn two(s: &str) -> (f64, f64) {
    match parse(s) {
        Some(Typed::Pair(a, b)) => (a, b),
        other => panic!("{s:?} -> {other:?}"),
    }
}

#[test]
fn test_length_fields_in_every_unit() {
    let _m = MetreDocument::new();
    let cases: &[(&str, f64)] = &[
        ("2", 2.0),
        ("30cm", 0.3),
        ("1500mm", 1.5),
        ("2m", 2.0),
        ("2\"", 2.0 * IN),
        ("2in", 2.0 * IN),
        ("2.5in", 2.5 * IN),
        ("1'", FT),
        ("1ft", FT),
        ("1.5ft", 1.5 * FT),
        ("1'6\"", FT + 6.0 * IN),
        ("1'6", FT + 6.0 * IN),
        ("0'6\"", 6.0 * IN),
        ("3/4\"", 0.75 * IN),
        ("1/2in", 0.5 * IN),
        ("1'3/4\"", FT + 0.75 * IN),
        ("-2\"", -2.0 * IN),
        ("-1'", -FT),
        (".5in", 0.5 * IN),
    ];
    for (text, metres) in cases {
        let got = parse_length_field(text).unwrap_or(f64::NAN);
        assert!((got - metres).abs() < 1e-9, "{text}: {got}");
    }
}

#[test]
fn test_nonsense_fields_are_rejected() {
    for bad in ["", "in", "\"", "2x", "3/0\"", "1''", "abc", "2mm3"] {
        assert_eq!(parse_length_field(bad), None, "{bad}");
    }
}

#[test]
fn test_rectangle_and_delta_mix_units() {
    let _m = MetreDocument::new();
    let (w, h) = two("2\";4\"");
    assert!((w - 2.0 * IN).abs() < 1e-9 && (h - 4.0 * IN).abs() < 1e-9);
    let Some(Typed::Triple(dx, dy, dz)) = parse("3,2;1'6\";10cm") else {
        panic!()
    };
    assert!(
        (dx - 3.2).abs() < 1e-9 && (dy - (FT + 6.0 * IN)).abs() < 1e-9 && (dz - 0.1).abs() < 1e-9
    );
    assert!((one("3/4\"") - 0.75 * IN).abs() < 1e-9);
    assert_eq!(parse("2r"), Some(Typed::Radius(2.0)));
    assert!(matches!(parse("3:12"), Some(Typed::Ratio(_))));
}

#[test]
fn test_the_buffer_takes_imperial_marks_only_after_a_digit() {
    let caps = Default::default();
    let mut vb = ValueBox::default();
    let key = |vb: &mut ValueBox, ch: char| vb.handle_key(&KeyEvent::typed(ch), caps);
    // a bare " or / or i is not input (I / F stay tool shortcuts)
    assert!(key(&mut vb, '"') && vb.buffer.is_empty());
    assert!(!key(&mut vb, 'i'));
    assert!(!key(&mut vb, 'f'));
    for ch in "2\"".chars() {
        key(&mut vb, ch);
    }
    assert_eq!(vb.buffer, "2\"");
    vb.buffer.clear();
    for ch in "1'6\"".chars() {
        key(&mut vb, ch);
    }
    assert_eq!(vb.buffer, "1'6\"");
    vb.buffer.clear();
    for ch in "3/4in".chars() {
        key(&mut vb, ch);
    }
    assert_eq!(vb.buffer, "3/4in");
}

#[test]
fn test_dimension_labels_can_read_in_inches_and_feet() {
    // Viewport._format_dim_value is format_length(style units, decimals).
    assert_eq!(format_length(2.0 * IN, "in", 2), "2.00\"");
    assert_eq!(format_length(FT + 6.0 * IN, "ft-in", 1), "1'6.0\"");
    assert_eq!(format_length(2.0 * FT, "ft", 2), "2.00'");
    assert_eq!(format_length(1.5, "m", 2), "1.50 m");
}

#[test]
fn test_mixed_numbers_the_usual_way() {
    let _m = MetreDocument::new();
    assert!((one("1 1/2\"") - 1.5 * IN).abs() < 1e-9);
    assert!((one("1-1/2\"") - 1.5 * IN).abs() < 1e-9);
    assert!((one("2 3/4in") - 2.75 * IN).abs() < 1e-9);
    assert!((one("-1 1/2\"") + 1.5 * IN).abs() < 1e-9);
    let (w, h) = two("1 1/2\";3 1/2\"");
    assert!((w - 1.5 * IN).abs() < 1e-9 && (h - 3.5 * IN).abs() < 1e-9);
    assert!((one("1'6 1/2\"") - (FT + 6.5 * IN)).abs() < 1e-9);
    assert!((one("1'6-1/2\"") - (FT + 6.5 * IN)).abs() < 1e-9);
    let (dx, dy) = two("2 1/2\";3");
    assert!((dx - 2.5 * IN).abs() < 1e-9 && (dy - 3.0).abs() < 1e-9);
    assert_eq!(parse("1 2"), Some(Typed::Pair(1.0, 2.0)));
    assert_eq!(parse_length_field("1-2"), None);
}

#[test]
fn test_fractional_inches_read_like_a_carpenter_writes_them() {
    assert_eq!(format_length(1.5 * IN, "in-frac", 2), "1 1/2\"");
    assert_eq!(format_length(0.75 * IN, "in-frac", 2), "3/4\"");
    assert_eq!(format_length(6.0 * IN, "in-frac", 2), "6\"");
    assert_eq!(format_length(FT + 6.5 * IN, "ft-in-frac", 2), "1'6 1/2\"");
    assert_eq!(format_length(2.0 * FT, "ft-in-frac", 2), "2'0\"");
    assert_eq!(format_length(11.99 * IN, "ft-in-frac", 0), "1'0\"");
    assert_eq!(format_length(1.03 * IN, "in-frac", 1), "1\"");
    assert_eq!(format_length(1.03 * IN, "in-frac", 4), "1 1/32\"");
    assert_eq!(format_length(-1.5 * IN, "in-frac", 2), "-1 1/2\"");
    assert_eq!(format_length(1.5, "m", 2), "1.50 m");
    assert_eq!(format_length(1.5, "mm", 0), "1500 mm");
}
