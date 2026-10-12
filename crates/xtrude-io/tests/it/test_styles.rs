// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Styles through .igz and saved views.

use crate::common;
use common::*;
use xtrude_core::camera::OrbitCamera;
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::Scene;
use xtrude_model::style::Style;

#[test]
fn test_igz_roundtrip_keeps_the_style() {
    let mut s = Scene::new();
    s.mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(1., 0., 0.))
        .unwrap();
    s.display_style = Style {
        name: "Monochrome".into(),
        face_mode: "monochrome".into(),
        background: [1.0, 1.0, 1.0],
        sky: false,
        ..Style::default()
    };
    let fresh = roundtrip(&s);
    assert_eq!(fresh.display_style.name, "Monochrome");
    assert_eq!(fresh.display_style.face_mode, "monochrome");
    assert!(!fresh.display_style.sky);
    let mut s2 = Scene::new();
    s2.mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(1., 0., 0.))
        .unwrap();
    let fresh2 = roundtrip(&s2);
    assert_eq!(fresh2.display_style.to_json(), Style::default().to_json());
}

#[test]
fn test_saved_view_remembers_the_style() {
    let mut s = Scene::new();
    let mut cam = OrbitCamera::new();
    s.display_style = Style {
        name: "Hidden line".into(),
        face_mode: "hidden_line".into(),
        ..Style::default()
    };
    let view = SavedView::capture("Top", &s, &cam);
    s.display_style = Style::default();
    view.apply(&mut s, &mut cam);
    assert_eq!(s.display_style.face_mode, "hidden_line");
    let again = SavedView::from_json(&view.to_json());
    s.display_style = Style::default();
    again.apply(&mut s, &mut cam);
    assert_eq!(s.display_style.name, "Hidden line");
}
