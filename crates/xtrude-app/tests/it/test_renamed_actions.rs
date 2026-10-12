// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Menu entries and actions named in the project's own words: the menus
//! and the right-click menu show the new labels, under the new titles.

use crate::common;

use common::{v3, window};
use serde_json::json;
use xtrude_app::window::{CtxItem, MainWindow, MenuItem};
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;
use xtrude_model::style::Style;

/// The label of the action `id`.
fn label(w: &MainWindow, id: &str) -> String {
    w.actions
        .get(id)
        .unwrap_or_else(|| panic!("no action {id:?}"))
        .text
        .clone()
}

/// The action ids of the menu-bar submenu titled `title`, anywhere.
fn submenu(w: &MainWindow, title: &str) -> Option<Vec<String>> {
    fn find(items: &[MenuItem], title: &str) -> Option<Vec<String>> {
        items.iter().find_map(|i| match i {
            MenuItem::Menu(t, sub) if t == title => Some(
                sub.iter()
                    .filter_map(|s| match s {
                        MenuItem::Action(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            MenuItem::Menu(_, sub) => find(sub, title),
            _ => None,
        })
    }
    w.menus.iter().find_map(|(_, items)| find(items, title))
}

/// The right-click submenu titled `title`: `(id, text)` per entry.
fn ctx_submenu(w: &MainWindow, title: &str) -> Option<Vec<(String, String)>> {
    w.context_menu().into_iter().find_map(|i| match i {
        CtxItem::Menu(t, sub) if t == title => Some(
            sub.into_iter()
                .filter_map(|s| match s {
                    CtxItem::Entry { id, text, .. } => Some((id, text)),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    })
}

fn select_a_face(w: &mut MainWindow) {
    let f = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    w.viewport
        .scene
        .select([EntityRef::Face(MeshId::LOOSE, f)], SelectMode::Replace);
}

#[test]
fn test_add_crossing_edges_in_the_menus() {
    let mut w = window();
    let ids = [
        "intersect:model",
        "intersect:selection",
        "intersect:context",
    ];
    let labels: Vec<String> = ids.iter().map(|id| label(&w, id)).collect();
    assert_eq!(
        labels,
        ["With Everything", "Within Selection", "Within Open Group"]
    );
    assert_eq!(
        submenu(&w, "Add Crossing Edges"),
        Some(ids.map(String::from).to_vec())
    );
    assert_eq!(submenu(&w, "Intersect Faces"), None);
    select_a_face(&mut w);
    let ctx = ctx_submenu(&w, "Add Crossing Edges").expect("the right-click entry");
    assert_eq!(
        ctx,
        ids.iter()
            .zip(&labels)
            .map(|(id, l)| (id.to_string(), l.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_make_independent_on_a_component() {
    let mut w = window();
    let mut proto = Mesh::new();
    proto.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let pid = add_mesh(&mut w.viewport.scene.doc.meshes, proto);
    let mut a = Group::new(pid, Some("Post"));
    a.xform = Some(DMat4::IDENTITY);
    let aid = a.id;
    w.viewport.scene.doc.groups.push(a);
    w.viewport
        .scene
        .select([EntityRef::Group(aid)], SelectMode::Replace);
    let entries: Vec<(String, String)> = w
        .context_menu()
        .into_iter()
        .filter_map(|i| match i {
            CtxItem::Entry { id, text, .. } => Some((id, text)),
            _ => None,
        })
        .collect();
    assert!(entries.contains(&("make_independent".into(), "Make Independent".into())));
    assert!(entries.iter().all(|(id, _)| id != "make_unique"));
    w.trigger("make_independent");
    assert!(w
        .viewport
        .scene
        .doc
        .find_group(aid)
        .unwrap()
        .xform
        .is_none());
}

#[test]
fn test_the_hidden_views_in_the_camera_menu() {
    let w = window();
    assert_eq!(label(&w, "show_hidden_groups"), "Hidden Groups");
    assert_eq!(
        label(&w, "show_hidden_edges_faces"),
        "Hidden Edges and Faces"
    );
    assert!(w.actions.get("hidden_objects").is_none());
    assert!(w.actions.get("hidden_geometry").is_none());
    let camera = &w.menus.iter().find(|(t, _)| t == "Camera").unwrap().1;
    for id in ["show_hidden_groups", "show_hidden_edges_faces"] {
        assert!(camera.contains(&MenuItem::Action(id.into())), "{id}");
    }
}

#[test]
fn test_a_style_with_profiles_and_back_edges_sets_the_renamed_toggles() {
    // the style keys are shared with IngeTrazo documents and stay
    let mut w = window();
    assert_eq!(label(&w, "style_silhouettes"), "Silhouette Edges");
    assert_eq!(label(&w, "style_hidden_lines"), "Hidden Lines");
    assert!(w.actions.get("style_profiles").is_none());
    assert!(w.actions.get("style_back_edges").is_none());
    let checked = |w: &MainWindow, id: &str| w.actions.get(id).unwrap().checked;
    for on in [true, false] {
        let raw = json!({"name": "Saved", "profiles": on, "back_edges": on});
        w.viewport.scene.display_style = Style::from_json(&raw);
        w.sync_style_menu();
        assert_eq!(checked(&w, "style_silhouettes"), on);
        assert_eq!(checked(&w, "style_hidden_lines"), on);
    }
    w.trigger("style_silhouettes");
    w.trigger("style_hidden_lines");
    let saved = w.viewport.scene.display_style.to_json();
    assert_eq!(
        (&saved["profiles"], &saved["back_edges"]),
        (&json!(true), &json!(true))
    );
}
