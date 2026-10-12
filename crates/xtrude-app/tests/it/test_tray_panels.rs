// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The app's one tray: Window ▸ Panels lists every section of it, hiding
//! the tray is remembered, a section's entry hides and shows it, asking
//! for a tray unfolds a folded sidebar, and Reset panel layout brings back
//! the factory layout.

use crate::common;

use common::window;
use xtrude_app::tray::{Tray, SECTIONS};
use xtrude_app::window::{CtxItem, MainWindow};

fn entry_checked(w: &MainWindow, id: &str) -> Option<bool> {
    w.panels_menu().into_iter().find_map(|i| match i {
        CtxItem::Entry { id: x, checked, .. } if x == id => checked,
        _ => None,
    })
}

#[test]
fn test_the_panels_menu_lists_every_section_then_the_reset() {
    let w = window();
    let titles: Vec<String> = w
        .panels_menu()
        .into_iter()
        .filter_map(|i| match i {
            CtxItem::Entry {
                text,
                checked: Some(_),
                ..
            } => Some(text),
            _ => None,
        })
        .collect();
    let sections: Vec<String> = SECTIONS.iter().map(|(_, t)| t.to_string()).collect();
    assert_eq!(titles, sections);
    let menu = w.panels_menu();
    assert_eq!(menu.len(), SECTIONS.len() + 2);
    assert!(matches!(menu[SECTIONS.len()], CtxItem::Separator));
    match &menu[SECTIONS.len() + 1] {
        CtxItem::Entry {
            id, text, checked, ..
        } => {
            assert_eq!(id, "reset_panel_layout");
            assert_eq!(text, "Reset panel layout");
            assert_eq!(*checked, None);
        }
        other => panic!("expected the reset entry, got {other:?}"),
    }
}

#[test]
fn test_hiding_a_tab_is_remembered_and_the_next_window_keeps_it_away() {
    let mut w = window();
    w.set_tray_shown("tray_properties", false);
    assert!(!w.tray_shown("tray_properties"));
    let other = MainWindow::new();
    assert!(!other.tray_shown("tray_properties"));
    w.trigger("tray_section:model_tree"); // a section brings its tray back
    assert!(w.tray_shown("tray_properties"));
}

#[test]
fn test_the_menu_entry_hides_and_shows_its_section() {
    let mut w = window();
    assert_eq!(entry_checked(&w, "tray_section:layers"), Some(true));
    w.trigger("tray_section:layers");
    assert!(!Tray::section_shown("layers"));
    assert!(w.tray_shown("tray_properties")); // the tray stays
    assert_eq!(entry_checked(&w, "tray_section:layers"), Some(false));
    assert_eq!(entry_checked(&w, "tray_section:saved_views"), Some(true));
    w.trigger("tray_section:layers");
    assert!(Tray::section_shown("layers"));
    assert_eq!(entry_checked(&w, "tray_section:layers"), Some(true));
}

#[test]
fn test_a_hidden_tray_unticks_every_section_and_a_tick_brings_it_back() {
    let mut w = window();
    w.set_tray_shown("tray_properties", false);
    for (key, _) in SECTIONS {
        assert_eq!(
            entry_checked(&w, &format!("tray_section:{key}")),
            Some(false)
        );
    }
    w.trigger("tray_section:parts");
    assert!(w.tray_shown("tray_properties"));
    for (key, _) in SECTIONS {
        assert_eq!(
            entry_checked(&w, &format!("tray_section:{key}")),
            Some(true)
        );
    }
}

#[test]
fn test_a_section_hidden_from_the_menu_stays_hidden_at_the_next_start() {
    let mut w = window();
    w.trigger("tray_section:colors");
    let next = MainWindow::new();
    assert!(!next.tray_section_visible("colors"));
    assert!(next.tray_section_visible("components"));
}

#[test]
fn test_showing_a_section_unfolds_a_folded_sidebar() {
    let mut w = window();
    w.set_tray_section_shown("saved_views", false);
    w.trigger("sidebar"); // folded
    assert!(!w.sidebar_visible());
    w.trigger("tray_section:saved_views");
    assert!(w.sidebar_visible() && w.tray_section_visible("saved_views"));
}

#[test]
fn test_the_panels_menu_follows_the_section_order() {
    let w = window();
    Tray::move_section("parts", Some("properties"));
    let ids: Vec<String> = w
        .panels_menu()
        .into_iter()
        .filter_map(|i| match i {
            CtxItem::Entry { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(ids[0], "tray_section:parts");
    assert_eq!(ids[1], "tray_section:properties");
}

#[test]
fn test_asking_for_a_tab_unfolds_a_folded_sidebar() {
    let mut w = window();
    w.trigger("sidebar"); // folded
    assert!(!w.sidebar_visible());
    w.set_tray_shown("tray_properties", true);
    assert!(w.sidebar_visible() && w.tray_shown("tray_properties"));
}

#[test]
fn test_reset_panel_layout_restores_order_and_visibility_and_keeps_folds() {
    let mut w = window();
    Tray::move_section("parts", Some("properties"));
    Tray::move_section("model_tree", None);
    w.trigger("tray_section:colors");
    w.trigger("tray_section:saved_views");
    Tray::set_section_open("layers", false);
    Tray::set_section_open("components", false);
    w.trigger("reset_panel_layout");
    let factory: Vec<&str> = SECTIONS.iter().map(|(k, _)| *k).collect();
    assert_eq!(Tray::section_order(), factory);
    for key in &factory {
        assert!(Tray::section_shown(key), "{key} hidden");
        assert!(w.tray_section_visible(key), "{key} not on screen");
        assert_eq!(
            entry_checked(&w, &format!("tray_section:{key}")),
            Some(true)
        );
    }
    assert!(!Tray::section_open("layers"));
    assert!(!Tray::section_open("components"));
    assert!(Tray::section_open("properties"));
}

#[test]
fn test_reset_panel_layout_shows_a_hidden_tray_in_a_folded_sidebar() {
    let mut w = window();
    w.set_tray_shown("tray_properties", false);
    w.trigger("sidebar"); // folded
    assert!(!w.sidebar_visible() && !w.tray_shown("tray_properties"));
    w.trigger("reset_panel_layout");
    assert!(w.sidebar_visible() && w.tray_shown("tray_properties"));
}

#[test]
fn test_the_reset_panel_layout_is_kept_at_the_next_start() {
    let mut w = window();
    Tray::move_section("parts", Some("properties"));
    w.trigger("tray_section:colors");
    w.trigger("reset_panel_layout");
    let next = MainWindow::new();
    let factory: Vec<&str> = SECTIONS.iter().map(|(k, _)| *k).collect();
    assert_eq!(Tray::section_order(), factory);
    assert!(SECTIONS.iter().all(|(k, _)| next.tray_section_visible(k)));
}
