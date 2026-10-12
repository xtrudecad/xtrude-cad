// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Light / dark chrome. The palettes are toolkit-neutral here; the shell turns
//! them into egui visuals each frame (so a desktop flip and a pinned mode are
//! both one `palette_for` away). egui restyles every frame and the icon
//! cache is keyed by ink.

use xtrude_app::{shell, theme};

#[test]
fn test_mode_resolution() {
    assert!(!theme::is_dark(theme::SYSTEM, Some(false)));
    assert!(theme::is_dark(theme::SYSTEM, Some(true)));
    // A desktop that says nothing keeps the historic dark chrome.
    assert!(theme::is_dark(theme::SYSTEM, None));
    for scheme in [Some(false), Some(true), None] {
        assert!(!theme::is_dark(theme::LIGHT, scheme));
        assert!(theme::is_dark(theme::DARK, scheme));
    }
}

#[test]
fn test_dark_is_the_default() {
    xtrude_model::settings::use_memory_store();
    xtrude_model::settings::remove("general/theme");
    assert_eq!(theme::saved_theme(), theme::DARK);
    theme::save_theme("sepia");
    assert_eq!(theme::saved_theme(), theme::DARK);
}

#[test]
fn test_pinned_modes_set_the_palette() {
    let light = theme::palette_for(Some(theme::LIGHT), Some(true));
    assert!(theme::lightness(light.window) >= 128);
    let dark = theme::palette_for(Some(theme::DARK), Some(false));
    assert!(theme::lightness(dark.window) < 128);
}

#[test]
fn test_both_palettes_define_the_shades_stylesheets_use() {
    for (p, dark) in [
        (theme::dark_palette(), true),
        (theme::light_palette(), false),
    ] {
        let text = theme::lightness(p.window_text);
        let window = theme::lightness(p.window);
        assert_eq!(text > window, dark);
        // the shades are all set (and distinct from the text)
        for shade in [p.light, p.midlight, p.mid, p.dark, p.shadow] {
            assert_ne!(shade, p.window_text);
        }
    }
}

#[test]
fn test_hovered_and_pressed_fills_stand_out_from_the_panel() {
    // A menu bar entry under the pointer, or pressed, paints its weak fill
    // over the panel: in both themes it must show.
    let l = |c: egui::Color32| theme::lightness([c.r(), c.g(), c.b()]);
    for (p, dark) in [
        (theme::dark_palette(), true),
        (theme::light_palette(), false),
    ] {
        let v = shell::visuals(dark, &p);
        let panel = l(v.panel_fill);
        let hovered = l(v.widgets.hovered.weak_bg_fill);
        let active = l(v.widgets.active.weak_bg_fill);
        assert!(hovered.abs_diff(panel) >= 20, "hover, dark: {dark}");
        assert!(active.abs_diff(panel) >= 20, "pressed, dark: {dark}");
        // pressing goes one step further than hovering
        assert!(active.abs_diff(panel) > hovered.abs_diff(panel));
    }
}

#[test]
fn test_muted_template_follows_the_theme() {
    let on_light = theme::muted(&theme::light_palette());
    let on_dark = theme::muted(&theme::dark_palette());
    assert_ne!(on_light, on_dark);
}

#[test]
fn test_system_mode_follows_a_desktop_flip() {
    xtrude_model::settings::use_memory_store();
    theme::save_theme(theme::SYSTEM);
    assert!(theme::lightness(theme::palette_for(None, Some(false)).window) >= 128);
    assert!(theme::lightness(theme::palette_for(None, Some(true)).window) < 128);
}

#[test]
fn test_a_pinned_mode_ignores_the_desktop() {
    xtrude_model::settings::use_memory_store();
    theme::save_theme(theme::LIGHT);
    assert!(theme::lightness(theme::palette_for(None, Some(true)).window) >= 128);
}

#[test]
fn test_typed_value_reads_on_the_typing_fill() {
    // The value box turns `typing_fill` while a value is typed and draws it
    // in `typing_text`: in both themes the digits must read, and typing
    // must show against the box at rest (`base`).
    for (p, dark) in [
        (theme::dark_palette(), true),
        (theme::light_palette(), false),
    ] {
        let fill = theme::lightness(p.typing_fill);
        let ink = theme::lightness(p.typing_text);
        assert!(fill.abs_diff(ink) >= 100, "contrast, dark: {dark}");
        assert_ne!(p.typing_fill, p.base, "dark: {dark}");
        assert!(
            fill.abs_diff(theme::lightness(p.base)) >= 20,
            "typing vs rest, dark: {dark}"
        );
    }
}
