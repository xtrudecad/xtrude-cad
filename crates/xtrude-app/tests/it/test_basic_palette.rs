// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Basic palette in the Colors panel: plain colours with plain
//! names, open on top of the library, with RAL Classic closed below it.

use crate::common;

use common::window;
use xtrude_app::tools::paint::paint_state;
use xtrude_app::tray::colors::{basic_colors, ral_colors};
use xtrude_app::tray::Tray;
use xtrude_model::i18n::tr;

#[test]
fn test_the_basic_palette_loads_with_plain_names() {
    let basic = basic_colors();
    assert_eq!(basic.len(), 16);
    let names: Vec<&str> = basic.iter().map(|s| s.tip.as_str()).collect();
    for n in ["White", "Black", "Grey", "Red", "Green", "Blue", "Yellow"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    for s in basic {
        assert_eq!(s.name.as_deref(), Some(s.tip.as_str()));
        assert!(s.rgb.iter().all(|v| (0.0..=1.0).contains(v)), "{s:?}");
    }
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "a name twice: {names:?}");
}

#[test]
fn test_basic_starts_open_and_ral_closed() {
    let w = window();
    let mut tray = Tray::default();
    tray.refresh(&w);
    assert!(tray.colors.basic_open);
    assert!(!tray.colors.library_open);
    assert_eq!(
        tray.colors.basic_title(),
        format!("{}  (16)", tr("Basic", &[]))
    );
    // RAL Classic is still there, whole.
    assert_eq!(ral_colors().len(), 213);
}

#[test]
fn test_a_basic_colour_paints_under_its_name() {
    let mut w = window();
    let mut tray = Tray::default();
    let white = basic_colors()
        .iter()
        .find(|s| s.tip == "White")
        .cloned()
        .expect("White");
    tray.colors
        .apply_color(&mut w, white.rgb, white.name.as_deref());
    let st = paint_state();
    assert_eq!(
        st.named_color.as_ref().map(|m| m.name.as_str()),
        Some("White")
    );
    assert_eq!(st.color, [1.0, 1.0, 1.0]);
}
