// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The tray's sections open as the user left them, keyed by the panel (not its
//! translated title); so do the closed ones and their order.

use serde_json::json;
use xtrude_app::tray::{drop_index, Tray, SECTIONS};
use xtrude_model::settings::{self, keys};

fn factory() -> Vec<&'static str> {
    SECTIONS.iter().map(|(k, _)| *k).collect()
}

#[test]
fn test_a_folded_section_stays_folded_and_the_others_stay_open() {
    settings::use_memory_store();
    assert!(Tray::section_open("saved_views")); // a first start: open
    Tray::set_section_open("saved_views", false); // the user folds it
    assert!(!Tray::section_open("saved_views")); // the next start
    assert!(Tray::section_open("layers")); // untouched ones open
    Tray::set_section_open("saved_views", true); // unfolded again…
    assert!(Tray::section_open("saved_views"));
}

#[test]
fn test_a_closed_section_stays_closed_and_keeps_its_fold() {
    settings::use_memory_store();
    assert!(SECTIONS.iter().all(|(k, _)| Tray::section_shown(k)));
    Tray::set_section_open("layers", false);
    Tray::set_section_shown("layers", false);
    assert!(!Tray::section_shown("layers"));
    assert!(Tray::section_shown("saved_views"));
    // stored by panel class, like the fold
    assert_eq!(
        settings::read(&keys::HIDDEN_TRAY_SECTIONS),
        json!(["LayersPanel"])
    );
    Tray::set_section_shown("layers", true);
    assert!(Tray::section_shown("layers"));
    assert!(!Tray::section_open("layers")); // still folded
}

#[test]
fn test_malformed_closed_sections_read_as_none_closed() {
    settings::use_memory_store();
    for bad in [
        json!("LayersPanel"),
        json!(42),
        json!({"a": 1}),
        json!(["NoSuchPanel", 3, null]),
    ] {
        settings::write(&keys::HIDDEN_TRAY_SECTIONS, &bad);
        assert!(
            SECTIONS.iter().all(|(k, _)| Tray::section_shown(k)),
            "{bad}"
        );
    }
    settings::write(
        &keys::HIDDEN_TRAY_SECTIONS,
        &json!(["NoSuchPanel", "PartsPanel", 7]),
    );
    assert!(!Tray::section_shown("parts"));
    assert!(Tray::section_shown("model_tree"));
}

#[test]
fn test_move_section_to_the_top_the_bottom_and_the_middle() {
    settings::use_memory_store();
    assert_eq!(Tray::section_order(), factory());
    Tray::move_section("parts", Some("properties"));
    assert_eq!(
        Tray::section_order(),
        [
            "parts",
            "properties",
            "model_tree",
            "layers",
            "saved_views",
            "colors",
            "components"
        ]
    );
    Tray::move_section("parts", None);
    assert_eq!(Tray::section_order(), factory());
    Tray::move_section("properties", Some("saved_views"));
    assert_eq!(
        Tray::section_order(),
        [
            "model_tree",
            "layers",
            "properties",
            "saved_views",
            "colors",
            "components",
            "parts"
        ]
    );
    // just above itself, or above the next one: no change
    Tray::move_section("saved_views", Some("saved_views"));
    Tray::move_section("layers", Some("properties"));
    assert_eq!(
        Tray::section_order(),
        [
            "model_tree",
            "layers",
            "properties",
            "saved_views",
            "colors",
            "components",
            "parts"
        ]
    );
    // saved by panel class
    let saved = settings::read(&keys::TRAY_SECTION_ORDER);
    assert_eq!(saved[0], json!("OutlinerPanel"));
}

#[test]
fn test_a_hidden_section_keeps_its_place_between_the_others() {
    settings::use_memory_store();
    Tray::set_section_shown("layers", false);
    Tray::move_section("properties", Some("saved_views"));
    assert_eq!(
        Tray::section_order(),
        [
            "model_tree",
            "layers",
            "properties",
            "saved_views",
            "colors",
            "components",
            "parts"
        ]
    );
    assert!(!Tray::section_shown("layers"));
}

#[test]
fn test_a_saved_order_with_unknown_twice_or_missing_sections_is_repaired() {
    settings::use_memory_store();
    settings::write(
        &keys::TRAY_SECTION_ORDER,
        &json!(["PartsPanel", "NoSuchPanel", "LayersPanel", "PartsPanel", 5]),
    );
    assert_eq!(
        Tray::section_order(),
        [
            "parts",
            "layers",
            "properties",
            "model_tree",
            "saved_views",
            "colors",
            "components"
        ]
    );
    settings::write(&keys::TRAY_SECTION_ORDER, &json!("garbage"));
    assert_eq!(Tray::section_order(), factory());
}

#[test]
fn test_drop_index_above_inside_and_below_each_section() {
    // three sections: 0..100, 100..140, 140..300
    let spans = [(0.0, 100.0), (100.0, 140.0), (140.0, 300.0)];
    assert_eq!(drop_index(&spans, -20.0), 0);
    assert_eq!(drop_index(&spans, 30.0), 0); // upper half of the first
    assert_eq!(drop_index(&spans, 70.0), 1); // lower half
    assert_eq!(drop_index(&spans, 115.0), 1);
    assert_eq!(drop_index(&spans, 125.0), 2);
    assert_eq!(drop_index(&spans, 200.0), 2);
    assert_eq!(drop_index(&spans, 250.0), 3);
    assert_eq!(drop_index(&spans, 900.0), 3);
    assert_eq!(drop_index(&[], 10.0), 0);
}

#[test]
fn test_the_colors_section_keeps_the_materials_panel_settings() {
    // it holds colours only; its fold, closing and place stay keyed by
    // the panel class, so settings saved before the rename still apply
    settings::use_memory_store();
    let (_, title) = SECTIONS.iter().find(|(k, _)| *k == "colors").unwrap();
    assert_eq!(*title, "Colors");
    settings::write(&keys::tray_collapsed("MaterialsPanel"), &true);
    settings::write(&keys::HIDDEN_TRAY_SECTIONS, &json!(["MaterialsPanel"]));
    assert!(!Tray::section_open("colors"));
    assert!(!Tray::section_shown("colors"));
}

#[test]
fn test_the_model_tree_section_keeps_its_stored_panel_settings() {
    // its settings were first saved under its former title: its fold,
    // closing and place stay keyed by that panel class
    settings::use_memory_store();
    let (_, title) = SECTIONS.iter().find(|(k, _)| *k == "model_tree").unwrap();
    assert_eq!(*title, "Model tree");
    settings::write(&keys::tray_collapsed("OutlinerPanel"), &true);
    settings::write(&keys::HIDDEN_TRAY_SECTIONS, &json!(["OutlinerPanel"]));
    settings::write(
        &keys::TRAY_SECTION_ORDER,
        &json!(["PartsPanel", "OutlinerPanel"]),
    );
    assert!(!Tray::section_open("model_tree"));
    assert!(!Tray::section_shown("model_tree"));
    assert_eq!(Tray::section_order()[..2], ["parts", "model_tree"]);
    assert!(SECTIONS.iter().all(|(k, _)| *k != "outliner"));
}

#[test]
fn test_the_properties_section_keeps_its_stored_panel_settings() {
    // its settings were first saved under its former title: its fold,
    // closing and place stay keyed by that panel class
    settings::use_memory_store();
    let (_, title) = SECTIONS.iter().find(|(k, _)| *k == "properties").unwrap();
    assert_eq!(*title, "Properties");
    settings::write(&keys::tray_collapsed("EntityInfoPanel"), &true);
    settings::write(&keys::HIDDEN_TRAY_SECTIONS, &json!(["EntityInfoPanel"]));
    settings::write(
        &keys::TRAY_SECTION_ORDER,
        &json!(["PartsPanel", "EntityInfoPanel"]),
    );
    assert!(!Tray::section_open("properties"));
    assert!(!Tray::section_shown("properties"));
    assert_eq!(Tray::section_order()[..2], ["parts", "properties"]);
    assert!(SECTIONS.iter().all(|(k, _)| *k != "entity_info"));
}

#[test]
fn test_the_saved_views_section_keeps_the_scenes_panel_settings() {
    // titled Scenes when its settings were first saved: its fold,
    // closing and place stay keyed by that panel class
    settings::use_memory_store();
    settings::write(&keys::tray_collapsed("ScenesPanel"), &true);
    settings::write(&keys::HIDDEN_TRAY_SECTIONS, &json!(["ScenesPanel"]));
    settings::write(
        &keys::TRAY_SECTION_ORDER,
        &json!(["PartsPanel", "ScenesPanel"]),
    );
    assert!(!Tray::section_open("saved_views"));
    assert!(!Tray::section_shown("saved_views"));
    assert_eq!(Tray::section_order()[..2], ["parts", "saved_views"]);
}
