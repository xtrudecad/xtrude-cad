// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Library tests: the user style library lives in the settings (`styles/user`,
//! a JSON list); `style_by_name` resolves built-ins first, then the library,
//! as copies. The panel half is in xtrude-app.

use serde_json::json;
use xtrude_model::scene::Scene;
use xtrude_model::settings;
use xtrude_model::style::{
    delete_user_style, effective_back_color, save_user_style, style_by_name, user_styles, Style,
    DEFAULT_BACK_COLOR,
};

fn fresh() {
    settings::use_memory_store();
    settings::remove("styles/user");
}

#[test]
fn test_save_load_round_trip() {
    fresh();
    let st = Style {
        name: "Build".into(),
        face_mode: "hidden_line".into(),
        edge_color: [0.1, 0.2, 0.3],
        sky: false,
        ..Style::default()
    };
    save_user_style(&st).unwrap();
    let loaded = user_styles();
    assert_eq!(
        loaded.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["Build"]
    );
    assert_eq!(loaded[0].face_mode, "hidden_line");
    assert_eq!(loaded[0].edge_color, [0.1, 0.2, 0.3]);
    assert!(!loaded[0].sky);
}

#[test]
fn test_save_replaces_by_name_and_delete() {
    fresh();
    let named = |n: &str, mode: &str| Style {
        name: n.into(),
        face_mode: mode.into(),
        ..Style::default()
    };
    save_user_style(&named("Build", "shaded")).unwrap();
    save_user_style(&named("Build", "wireframe")).unwrap();
    save_user_style(&named("Sketch", "shaded")).unwrap();
    let names: Vec<String> = user_styles().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["Build", "Sketch"]);
    assert_eq!(user_styles()[0].face_mode, "wireframe"); // replaced, not doubled
    assert!(delete_user_style("Build"));
    assert!(!delete_user_style("Build")); // already gone
    let names: Vec<String> = user_styles().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["Sketch"]);
}

#[test]
fn test_builtin_name_is_refused() {
    fresh();
    assert!(save_user_style(&Style {
        name: "Hidden line".into(),
        ..Style::default()
    })
    .is_err());
    assert!(user_styles().is_empty());
}

#[test]
fn test_style_by_name_resolves_user_styles_as_copies() {
    fresh();
    save_user_style(&Style {
        name: "Build".into(),
        face_mode: "monochrome".into(),
        ..Style::default()
    })
    .unwrap();
    let mut got = style_by_name("Build").unwrap();
    assert_eq!(got.face_mode, "monochrome");
    got.face_mode = "xray".into(); // a COPY: no write-back
    assert_eq!(style_by_name("Build").unwrap().face_mode, "monochrome");
    assert_eq!(style_by_name("Wireframe").unwrap().face_mode, "wireframe");
    assert!(style_by_name("missing").is_none());
}

#[test]
fn test_broken_settings_entry_is_dropped() {
    fresh();
    settings::set("styles/user", json!("{this is not json"));
    assert!(user_styles().is_empty());
}

#[test]
fn test_sky_and_ground_colors_are_style_data() {
    fresh();
    save_user_style(&Style {
        name: "Evening".into(),
        sky_color: [0.9, 0.7, 0.5],
        ground_color: [0.4, 0.3, 0.2],
        ..Style::default()
    })
    .unwrap();
    let got = style_by_name("Evening").unwrap();
    assert_eq!(got.sky_color, [0.9, 0.7, 0.5]);
    assert_eq!(got.ground_color, [0.4, 0.3, 0.2]);
    // An old style without the fields gets the viewport's former constants.
    let legacy = Style::from_json(&json!({"name": "old"}));
    assert_eq!(legacy.sky_color, [0.925, 0.935, 0.945]);
    assert_eq!(legacy.ground_color, [0.815, 0.820, 0.815]);
}

#[test]
fn test_back_color_is_style_data_and_round_trips() {
    fresh();
    assert!(Style::default().back_color.is_none());
    assert_eq!(
        effective_back_color(&Style::default(), None),
        DEFAULT_BACK_COLOR
    );
    let st = Style {
        name: "Interior".into(),
        back_color: Some([0.9, 0.2, 0.1]),
        ..Style::default()
    };
    assert_eq!(
        Style::from_json(&st.to_json()).back_color,
        Some([0.9, 0.2, 0.1])
    );
    assert!(Style::from_json(&Style::default().to_json())
        .back_color
        .is_none());
    save_user_style(&st).unwrap();
    assert_eq!(
        style_by_name("Interior").unwrap().back_color,
        Some([0.9, 0.2, 0.1])
    );
    // Old documents (no key) and junk values fall back to automatic.
    assert!(Style::from_json(&json!({})).back_color.is_none());
    assert!(Style::from_json(&json!({"back_color": "reddish"}))
        .back_color
        .is_none());
    assert!(Style::from_json(&json!({"back_color": [1, 2]}))
        .back_color
        .is_none());
}

#[test]
fn test_back_color_precedence_style_then_document_then_default() {
    let mut scene = Scene::new();
    assert_eq!(
        effective_back_color(&scene.display_style, scene.back_face_color),
        DEFAULT_BACK_COLOR
    );
    scene.back_face_color = Some([0.5, 0.5, 0.5]);
    assert_eq!(
        effective_back_color(&scene.display_style, scene.back_face_color),
        [0.5, 0.5, 0.5]
    );
    scene.display_style.back_color = Some([0.1, 0.8, 0.3]);
    assert_eq!(
        effective_back_color(&scene.display_style, scene.back_face_color),
        [0.1, 0.8, 0.3]
    );
}
