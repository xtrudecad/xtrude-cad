// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Real upstream IngeTrazo documents (one carrying data of features Xtrude CAD
//! dropped) open, and are saved as Xtrude CAD documents without the removed
//! blocks (the types have no such fields).

use crate::common;
use common::*;
use std::collections::HashSet;
use xtrude_io::{document, igz};
use xtrude_model::group::iter_placements;
use xtrude_model::scene::Scene;

const REMOVED_KEYS: [&str; 8] = [
    "georef",
    "tile_layer",
    "geo_paths",
    "geo_points",
    "photo_mesh",
    "compositions",
    "custom_scales",
    "shadows",
];

fn fixture() -> std::path::PathBuf {
    data("upstream_removed_features.igz")
}

#[test]
fn test_the_fixture_really_carries_the_removed_blocks() {
    let p = payload(&fixture());
    let keys: HashSet<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
    assert!(REMOVED_KEYS.iter().all(|k| keys.contains(k)));
    assert!(p["faces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f.get("ifc").is_some()));
    assert!(p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g.get("ifc").is_some()));
    assert!(p["materials"][0].get("finish").is_some());
    assert_eq!(p["display_style"]["name"], "Architectural");
    let view = &p["saved_views"][0];
    assert!(view.get("georef").is_some() && view.get("shadows").is_some());
}

#[test]
fn test_an_upstream_document_with_removed_features_opens() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &fixture(), None).unwrap();
    assert_eq!(s.mesh().faces.len(), 6);
    let names: Vec<&str> = s.doc.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Soporte servo", "Ingeniero"]);
    assert_eq!(s.doc.mesh(s.doc.groups[0].mesh).faces.len(), 6);
    assert_eq!(s.doc.dimensions.len(), 1);
    assert_eq!(s.doc.text_labels.len(), 1);
    assert_eq!(
        s.saved_views
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>(),
        ["Planta"]
    );
    assert!(s.doc.materials.contains_key("PLA naranja"));
    assert!(s
        .mesh()
        .faces
        .iter()
        .all(|(_, f)| f.attrs.paint.mat.as_deref() == Some("PLA naranja")));
}

#[test]
fn test_saving_it_again_writes_none_of_the_removed_blocks() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &fixture(), None).unwrap();
    let tmp = TmpDir::new();
    let out = tmp.join("again.xtrude");
    document::save_scene(&s, &out).unwrap();
    let p = payload(&out);
    let keys: HashSet<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
    assert!(REMOVED_KEYS.iter().all(|k| !keys.contains(k)));
    assert!(!p["faces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f.get("ifc").is_some()));
    assert!(!p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g.get("ifc").is_some()));
    assert!(p["materials"][0].get("finish").is_none());
    let view = &p["saved_views"][0];
    assert!(view.get("georef").is_none() && view.get("shadows").is_none());

    let mut back = Scene::new();
    document::load_into(&mut back, &out, None).unwrap();
    assert_eq!(back.mesh().faces.len(), 6);
    let names: Vec<&str> = back.doc.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Soporte servo", "Ingeniero"]);
}

#[test]
fn test_an_upstream_example_document_opens() {
    let path = data("banca-pergola-yanque.igz");
    assert!(payload(&path).get("compositions").is_some());
    let mut s = Scene::new();
    igz::load_into(&mut s, &path, None).unwrap();
    assert_eq!(s.doc.groups.len(), 9);
    assert_eq!(s.saved_views.len(), 5);
    assert_eq!(s.doc.dimensions.len(), 4);
    let faces: usize = s
        .doc
        .groups
        .iter()
        .flat_map(|top| iter_placements(top, None))
        .map(|(g, _)| s.doc.mesh(g.mesh).faces.len())
        .sum();
    assert!(faces > 0);
}
