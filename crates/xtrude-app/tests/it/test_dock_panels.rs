// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The left dock: Model and Properties shown and Layers hidden on a first
//! start; Window ▸ Panels lists them, hides and shows each and remembers
//! it; a panel's fold is remembered; Reset panel layout brings back the
//! factory layout; the drop-down panels open from their menu entries;
//! and a 0.1.0 tray layout is left unread.

use crate::common;

use common::window;
use serde_json::{json, Value};
use xtrude_app::tray::{Tray, DOCK_PANELS};
use xtrude_app::window::{CtxItem, MainWindow, MenuItem, TOOLBAR_PANELS};
use xtrude_model::settings::{self, keys, Key};

fn entry_checked(w: &MainWindow, id: &str) -> Option<bool> {
    w.panels_menu().into_iter().find_map(|i| match i {
        CtxItem::Entry { id: x, checked, .. } if x == id => checked,
        _ => None,
    })
}

fn shown() -> Vec<&'static str> {
    Tray::shown_panels()
}

#[test]
fn test_the_dock_holds_model_properties_and_layers_in_that_order() {
    let titles: Vec<(&str, &str)> = DOCK_PANELS.to_vec();
    assert_eq!(
        titles,
        [
            ("model", "Model"),
            ("properties", "Properties"),
            ("layers", "Layers")
        ]
    );
}

#[test]
fn test_a_first_start_shows_model_and_properties_and_hides_layers() {
    let w = window();
    assert_eq!(shown(), ["model", "properties"]);
    assert!(w.sidebar_visible());
    assert_eq!(entry_checked(&w, "dock_panel:model"), Some(true));
    assert_eq!(entry_checked(&w, "dock_panel:properties"), Some(true));
    assert_eq!(entry_checked(&w, "dock_panel:layers"), Some(false));
    assert!(DOCK_PANELS.iter().all(|(k, _)| Tray::panel_open(k)));
}

#[test]
fn test_the_panels_menu_lists_every_dock_panel_then_the_reset() {
    let w = window();
    let menu = w.panels_menu();
    let titles: Vec<String> = menu
        .iter()
        .filter_map(|i| match i {
            CtxItem::Entry {
                text,
                checked: Some(_),
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["Model", "Properties", "Layers"]);
    assert_eq!(menu.len(), DOCK_PANELS.len() + 2);
    assert!(matches!(menu[DOCK_PANELS.len()], CtxItem::Separator));
    match &menu[DOCK_PANELS.len() + 1] {
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
fn test_the_menu_entry_docks_layers_and_hides_it_again() {
    let mut w = window();
    w.trigger("dock_panel:layers");
    assert_eq!(shown(), ["model", "properties", "layers"]);
    assert_eq!(entry_checked(&w, "dock_panel:layers"), Some(true));
    w.trigger("dock_panel:layers");
    assert_eq!(shown(), ["model", "properties"]);
    w.trigger("dock_panel:model");
    assert_eq!(shown(), ["properties"]);
    assert_eq!(entry_checked(&w, "dock_panel:model"), Some(false));
}

#[test]
fn test_a_panel_hidden_or_docked_stays_so_at_the_next_start() {
    let mut w = window();
    w.trigger("dock_panel:properties");
    w.trigger("dock_panel:layers");
    let _next = MainWindow::new();
    assert_eq!(shown(), ["model", "layers"]);
    // stored by panel class
    assert_eq!(
        settings::read(&keys::HIDDEN_DOCK_PANELS),
        json!(["PropertiesPanel"])
    );
}

#[test]
fn test_showing_a_panel_unfolds_a_folded_dock() {
    let mut w = window();
    w.trigger("sidebar"); // folded
    assert!(!w.sidebar_visible());
    w.trigger("dock_panel:layers");
    assert!(w.sidebar_visible() && Tray::panel_shown("layers"));
    w.trigger("sidebar");
    w.trigger("dock_panel:layers"); // hiding leaves the fold
    assert!(!w.sidebar_visible());
}

#[test]
fn test_a_folded_dock_opens_unfolded() {
    let mut w = window();
    w.set_sidebar_visible(false);
    assert!(!w.sidebar_visible());
    assert!(MainWindow::new().sidebar_visible());
}

#[test]
fn test_a_panels_fold_is_remembered_and_kept_while_hidden() {
    let _w = window();
    Tray::set_panel_open("properties", false);
    assert!(!Tray::panel_open("properties"));
    assert!(Tray::panel_open("model"));
    assert!(settings::read(&keys::dock_collapsed("PropertiesPanel")));
    Tray::set_panel_shown("properties", false);
    Tray::set_panel_shown("properties", true);
    assert!(!Tray::panel_open("properties"));
    Tray::set_panel_open("properties", true);
    assert!(Tray::panel_open("properties"));
}

#[test]
fn test_reset_panel_layout_restores_the_factory_panels_and_keeps_folds() {
    let mut w = window();
    w.trigger("dock_panel:model");
    w.trigger("dock_panel:layers");
    Tray::set_panel_open("properties", false);
    w.trigger("sidebar"); // folded
    w.trigger("reset_panel_layout");
    assert_eq!(shown(), ["model", "properties"]);
    assert!(w.sidebar_visible());
    assert!(!Tray::panel_open("properties"));
    let _next = MainWindow::new();
    assert_eq!(shown(), ["model", "properties"]);
}

#[test]
fn test_malformed_hidden_panels_read_as_none_hidden() {
    let _w = window();
    for bad in [json!("LayersPanel"), json!(42), json!({"a": 1}), json!([3])] {
        settings::write(&keys::HIDDEN_DOCK_PANELS, &bad);
        assert_eq!(shown(), ["model", "properties", "layers"], "{bad}");
    }
    settings::write(
        &keys::HIDDEN_DOCK_PANELS,
        &json!(["NoSuchPanel", "ModelPanel", 7, "ModelPanel"]),
    );
    assert_eq!(shown(), ["properties", "layers"]);
}

#[test]
fn test_a_0_1_0_tray_layout_is_left_unread_and_the_factory_dock_loads() {
    let _w = window();
    // what 0.1.0 saved: its right-hand tray hidden, Properties and Model
    // tree closed, Layers shown first, Properties folded
    let old: [(&'static str, Value); 4] = [
        ("ui/hidden_trays", json!(["tray_properties"])),
        (
            "ui/hidden_tray_sections",
            json!(["EntityInfoPanel", "OutlinerPanel"]),
        ),
        (
            "ui/tray_section_order",
            json!(["LayersPanel", "PartsPanel", "EntityInfoPanel"]),
        ),
        ("tray/collapsed/EntityInfoPanel", json!(true)),
    ];
    for (name, value) in &old {
        settings::write(&Key::new(name, || Value::Null), value);
    }
    settings::write(
        &Key::named("tray/collapsed/LayersPanel".into(), || false),
        &true,
    );
    let w = MainWindow::new();
    assert!(w.sidebar_visible());
    assert_eq!(shown(), ["model", "properties"]);
    assert!(DOCK_PANELS.iter().all(|(k, _)| Tray::panel_open(k)));
    // left as they were, not deleted
    for (name, value) in &old {
        assert_eq!(&settings::read(&Key::new(name, || Value::Null)), value);
    }
}

#[test]
fn test_the_drop_down_panels_open_from_their_menu_entries() {
    let mut w = window();
    for key in ["colors", "layers", "saved_views", "exploded_view"] {
        w.panel_requested = None;
        assert!(w.trigger(&format!("panel:{key}")), "{key}");
        assert_eq!(w.panel_requested, Some(key));
        assert!(TOOLBAR_PANELS.iter().any(|(k, _)| *k == key));
    }
    w.panel_requested = None;
    assert!(!w.trigger("panel:no_such_panel"));
    assert_eq!(w.panel_requested, None);
}

/// The action ids of the top-level menu titled `title`, submenus included.
fn menu_ids(w: &MainWindow, title: &str) -> Vec<String> {
    fn walk(items: &[MenuItem], out: &mut Vec<String>) {
        for i in items {
            match i {
                MenuItem::Action(id) => out.push(id.clone()),
                MenuItem::Menu(_, sub) => walk(sub, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    if let Some((_, items)) = w.menus.iter().find(|(t, _)| t.replace('&', "") == title) {
        walk(items, &mut out);
    }
    out
}

#[test]
fn test_view_opens_layers_saved_views_and_the_exploded_view_and_model_opens_colors() {
    let w = window();
    let view = menu_ids(&w, "View");
    for id in ["panel:layers", "panel:saved_views", "panel:exploded_view"] {
        assert!(view.iter().any(|x| x == id), "{id} not in View");
    }
    let model = menu_ids(&w, "Model");
    assert!(model.iter().any(|x| x == "panel:colors"));
    let colors = w.actions.get("panel:colors").unwrap();
    assert_eq!(colors.text, "Colors…");
}
