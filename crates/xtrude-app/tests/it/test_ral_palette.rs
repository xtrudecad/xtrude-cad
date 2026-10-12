// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! RAL Classic in the Colors section. A colour picked there paints under
//! the name of the reference a painter can buy, the swatches are built only
//! when the section opens, and every colour in it has a name.

use crate::common;

use common::window;
use xtrude_app::tools::paint::paint_state;
use xtrude_app::tray::Tray;
use xtrude_model::i18n::tr;

#[test]
fn test_picking_a_ral_colour_paints_it_with_its_name() {
    let mut w = window();
    let mut tray = Tray::default();
    let panel = &mut tray.colors;
    panel.refresh_in_model(&w);
    // Everything lives in the one library section, named after the
    // palette (the tray section itself is «Colors») — no heading per RAL
    // family to click through.
    let title = panel.library_title();
    assert_eq!(title, format!("{}  (213)", tr("RAL Classic", &[])));
    panel.set_library_open(true); // the swatches build on open
    let swatch = panel
        .library
        .iter()
        .find(|s| s.tip.starts_with("RAL 7035"))
        .cloned()
        .expect("RAL 7035");
    assert_eq!(swatch.tip, "RAL 7035 · Light grey");
    panel.apply_color(&mut w, swatch.rgb, swatch.name.as_deref());
    let st = paint_state();
    assert_eq!(
        st.named_color.as_ref().map(|m| m.name.as_str()),
        Some("RAL 7035 Light grey")
    );
    // sRGB of RAL 7035, as the standard gives it.
    let rgb: Vec<i64> = st
        .color
        .iter()
        .map(|v| (v * 255.0).round() as i64)
        .collect();
    assert_eq!(rgb, [203, 208, 204]);
}

#[test]
fn test_the_families_build_only_when_opened() {
    // Two hundred colours must not be built at start-up to fill a section
    // that is closed.
    let w = window();
    let mut tray = Tray::default();
    tray.refresh(&w);
    assert!(!tray.colors.library_open);
    assert!(
        tray.colors.library.is_empty(),
        "the RAL swatches were built before opening"
    );
}

#[test]
fn test_every_colour_in_the_tray_has_a_name() {
    // Beside 213 colours that each carry a reference a painter can buy, a
    // nameless square is only confusing.
    let w = window();
    let mut tray = Tray::default();
    tray.refresh(&w);
    tray.colors.set_library_open(true);
    let lib = &tray.colors.library;
    let nameless: Vec<&str> = lib
        .iter()
        .filter(|s| !s.tip.starts_with("RAL"))
        .map(|s| s.tip.as_str())
        .take(5)
        .collect();
    assert!(nameless.is_empty(), "{nameless:?}");
    assert_eq!(lib.iter().filter(|s| s.tip.starts_with("RAL")).count(), 213);
}
