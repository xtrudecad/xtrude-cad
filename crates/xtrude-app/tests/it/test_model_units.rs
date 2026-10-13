// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The document carries its units; a bare number is typed in them and every
//! readout shows them. The document round trip is in xtrude-io.

use crate::common;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::Tool;
use xtrude_app::value_box::parse_length_field;
use xtrude_model::units::{
    bind, default_model_units, fmt_area, fmt_len, fmt_pair, fmt_volume, unit_label, Units,
};

/// A fresh document's units bound, restored after.
struct Bound;

impl Bound {
    fn new() -> Bound {
        bind(&default_model_units());
        Bound
    }

    fn set(&self, length: &str, precision: i64) {
        bind(&Units::new(length, precision));
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        bind(&default_model_units());
    }
}

#[test]
fn test_readouts_follow_the_models_units() {
    let b = Bound::new();
    assert_eq!(fmt_len(2.5), "2500.00 mm"); // the default
    b.set("m", 2);
    assert_eq!(fmt_len(2.5), "2.50 m");
    assert_eq!(fmt_pair(3.0, 2.0), "3.00 × 2.00 m");
    assert_eq!(fmt_area(4.0), "4.00 m²");
    b.set("mm", 0);
    assert_eq!(fmt_len(2.5), "2500 mm");
    assert_eq!(fmt_pair(3.0, 2.0), "3000 × 2000 mm");
    assert_eq!(fmt_area(4.0), "4000000 mm²");
    b.set("cm", 1);
    assert_eq!(fmt_len(0.256), "25.6 cm");
    b.set("ft-in", 0);
    assert_eq!(fmt_len(0.3048 * 2.0 + 0.0254 * 6.0), "2'6\"");
    assert_eq!(fmt_area(0.3048 * 0.3048), "1 ft²");
}

#[test]
fn test_a_volume_reads_in_cm3_in_mm_and_cm_documents() {
    let b = Bound::new();
    assert_eq!(fmt_volume(8e-6), "8.00 cm³"); // the default: mm
    b.set("cm", 1);
    assert_eq!(fmt_volume(2.5e-5), "25.0 cm³");
}

#[test]
fn test_a_volume_reads_in_m3_in_metre_documents() {
    let b = Bound::new();
    b.set("m", 3);
    assert_eq!(fmt_volume(0.25), "0.250 m³");
}

#[test]
fn test_a_volume_reads_in_in3_in_inch_documents() {
    let b = Bound::new();
    let inch3 = 0.0254f64.powi(3);
    b.set("in", 2);
    assert_eq!(fmt_volume(8.0 * inch3), "8.00 in³");
    b.set("in-frac", 1);
    assert_eq!(fmt_volume(1.5 * inch3), "1.5 in³");
}

#[test]
fn test_a_volume_reads_in_ft3_in_feet_documents() {
    let b = Bound::new();
    let foot3 = 0.3048f64.powi(3);
    b.set("ft", 1);
    assert_eq!(fmt_volume(foot3), "1.0 ft³");
    b.set("ft-in", 0);
    assert_eq!(fmt_volume(2.0 * foot3), "2 ft³");
    b.set("ft-in-frac", 0);
    assert_eq!(fmt_volume(3.0 * foot3), "3 ft³");
}

fn parse(s: &str) -> f64 {
    parse_length_field(s).unwrap_or_else(|| panic!("{s}"))
}

#[test]
fn test_a_bare_number_is_typed_in_the_models_unit() {
    let b = Bound::new();
    assert!((parse("2") - 0.002).abs() < 1e-12); // the default: mm
    b.set("m", 2);
    assert!((parse("2") - 2.0).abs() < 1e-9);
    b.set("mm", 0);
    assert!((parse("2") - 0.002).abs() < 1e-12);
    assert!((parse("2m") - 2.0).abs() < 1e-9); // an explicit unit wins
    assert!((parse("-30") + 0.030).abs() < 1e-12);
    b.set("ft-in", 0);
    assert!((parse("6") - 6.0 * 0.0254).abs() < 1e-9); // inches, as in .skp
}

#[test]
fn test_a_tool_label_reads_in_the_models_units() {
    let b = Bound::new();
    let vp = stub();
    let mut t = RectangleTool::new();
    t.start_point = Some(DVec3::ZERO);
    t.hover_point = Some(DVec3::new(5., 4., 0.));
    let label = |t: &RectangleTool| t.value_label(&vp).unwrap().0;
    assert_eq!(label(&t), "5000.00 × 4000.00 mm"); // the default
    b.set("m", 2);
    assert_eq!(label(&t), "5.00 × 4.00 m");
    b.set("cm", 0);
    assert_eq!(label(&t), "500 × 400 cm");
}

#[test]
fn test_the_viewport_binds_its_scene_and_the_dialog_reads_it() {
    let _b = Bound::new();
    let mut vp = stub();
    assert_eq!(vp.scene.units, Units::new("mm", 2));
    vp.scene.units = Units::new("in", 1);
    // The scene binds a copy of its units, re-bound whenever they change.
    vp.scene.bind_units();
    assert_eq!(fmt_len(0.0254), "1.0\"");
}

#[test]
fn test_unit_labels_are_english() {
    assert_eq!(unit_label("m"), "Metres (m)");
    assert_eq!(unit_label("ft-in-frac"), "Fractional feet and inches");
}
