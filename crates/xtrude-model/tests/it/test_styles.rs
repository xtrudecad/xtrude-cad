// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Styles: the document half (the .igz and saved view tests live in
//! xtrude-io).

use serde_json::json;
use xtrude_model::scene::Scene;
use xtrude_model::style::{builtin_styles, style_by_name, Style, FACE_MODES};

#[test]
fn test_style_roundtrip_and_defaults() {
    let s = Style::default();
    assert!(s.face_mode == "shaded" && s.edges && s.profiles && s.sky);
    assert_eq!(Style::from_json(&s.to_json()).to_json(), s.to_json());
    assert_eq!(
        Style::from_json(&json!({"face_mode": "no_such_mode"})).face_mode,
        "shaded"
    );
    assert_eq!(
        Style::from_json(&json!({"face_mode": "textures"})).face_mode,
        "shaded"
    );
    let mut c = s.clone();
    c.edges = false;
    assert!(s.edges);
}

#[test]
fn test_builtin_presets_cover_the_classics() {
    let presets = builtin_styles();
    let names: std::collections::HashSet<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    for n in ["Default", "Hidden line", "Monochrome", "Wireframe", "X-ray"] {
        assert!(names.contains(n), "{n}");
    }
    assert!(presets
        .iter()
        .all(|p| FACE_MODES.contains(&p.face_mode.as_str())));
    let hidden = presets.iter().find(|p| p.name == "Hidden line").unwrap();
    assert_eq!(hidden.face_mode, "hidden_line");
    assert!(hidden.background == [1.0, 1.0, 1.0] && !hidden.sky);
}

#[test]
fn test_scene_carries_a_style_and_clear_resets_it() {
    let mut s = Scene::new();
    assert_eq!(s.display_style.face_mode, "shaded");
    s.display_style = Style {
        name: "Hidden line".into(),
        face_mode: "hidden_line".into(),
        ..Style::default()
    };
    s.mesh_mut()
        .add_edge(xtrude_core::DVec3::ZERO, xtrude_core::DVec3::X)
        .unwrap();
    s.clear();
    assert_eq!(s.display_style.face_mode, "shaded");
}

#[test]
fn test_style_by_name_returns_copies() {
    let mut a = style_by_name("Hidden line").unwrap();
    assert_eq!(a.face_mode, "hidden_line");
    assert!(a.background == [1.0, 1.0, 1.0] && !a.sky);
    a.edges = false;
    assert!(style_by_name("Hidden line").unwrap().edges);
    assert!(style_by_name("Missing").is_none());
}
