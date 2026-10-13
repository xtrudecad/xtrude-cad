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
use xtrude_model::print_material::{default_print_material, PrintMaterial};
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

#[test]
fn test_upstream_documents_open_in_the_default_print_material() {
    for name in ["upstream_removed_features.igz", "banca-pergola-yanque.igz"] {
        let mut s = Scene::new();
        s.doc.print_material = PrintMaterial::new("Resin", 1100.0);
        igz::load_into(&mut s, &data(name), None).unwrap();
        assert_eq!(s.doc.print_material, default_print_material(), "{name}");
        assert!(
            s.doc
                .all_groups()
                .iter()
                .all(|g| g.print_material.is_none()),
            "{name}"
        );
    }
}

fn new_keys() -> std::path::PathBuf {
    data("ingetrazo-0.5.8-new-keys.igz")
}

fn format3() -> std::path::PathBuf {
    data("ingetrazo-0.5.8-format3.igz")
}

fn near(a: xtrude_core::DVec3, b: xtrude_core::DVec3) -> bool {
    (a - b).abs().max_element() < 1e-5
}

/// Whether `mesh` has a vertex at `p` (within 1e-5 m).
fn has_vertex(mesh: &xtrude_core::Mesh, p: xtrude_core::DVec3) -> bool {
    mesh.vertex_ids().into_iter().any(|v| near(mesh.pos(v), p))
}

/// Vertices, edges and faces of `mesh`.
fn counts(mesh: &xtrude_core::Mesh) -> [usize; 3] {
    [mesh.vertex_ids().len(), mesh.edges.len(), mesh.faces.len()]
}

#[test]
fn test_the_0_5_8_fixtures_carry_what_they_test() {
    let doc = document_json(&new_keys());
    assert_eq!(doc["app_version"], "0.5.8");
    let p = &doc["scene"];
    assert_eq!(p["groups"][0]["locked"], true);
    let kept: Vec<&str> = p["kept_components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["name"].as_str().unwrap())
        .collect();
    assert_eq!(kept, ["Bracket", "Spare peg"]);
    for key in ["area", "volume", "angle", "angle_precision", "decimal"] {
        assert!(p["units"].get(key).is_some(), "{key}");
    }
    assert_eq!(p["section_planes"][0]["fill"]["pattern"], "hatch");

    let doc = document_json(&format3());
    assert_eq!(
        (doc["igz_format"].as_i64(), doc["app_version"].as_str()),
        (Some(3), Some("0.5.8"))
    );
    let p = &doc["scene"];
    assert!(p.get("faces").is_none() && p.get("edges").is_none());
    assert!(p["geom"]["member"].is_string());
    assert!(p["groups"][0]["geom"]["member"].is_string());
    assert!(p["protos"][0]["geom"]["member"].is_string());
}

#[test]
fn test_an_ingetrazo_0_5_8_document_with_the_new_keys_opens() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &new_keys(), None).unwrap();
    assert_eq!(s.load_repairs, 0);
    assert_eq!(s.mesh().faces.len(), 6);
    let names: Vec<&str> = s.doc.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Locked base", "Bracket"]);
    let layers: Vec<&str> = s.doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(layers, ["Layer 0", "Parts"]);
    // the locked group opens as any group: shown, on its layer, painted
    let base = &s.doc.groups[0];
    assert!(!base.hidden && !base.is_component());
    assert_eq!(base.layer.as_deref(), Some("Parts"));
    let painted: Vec<_> = s
        .doc
        .mesh(base.mesh)
        .faces
        .iter()
        .filter_map(|(_, f)| f.attrs.paint.color)
        .collect();
    assert_eq!(painted, [[0.8, 0.2, 0.1]]);
    let bracket = &s.doc.groups[1];
    assert!(bracket.is_component());
    assert_eq!(s.doc.mesh(bracket.mesh).faces.len(), 6);
    // the kept definitions are dropped, their prototype with them: the
    // loose mesh, the group's and the bracket's
    assert_eq!(s.doc.meshes.len(), 3);
    assert_eq!(s.doc.all_groups().len(), 2);
    // the length unit and its precision; the rest follows the length
    assert_eq!((s.units.length.as_str(), s.units.precision), ("mm", 1));
    // the section plane, without its own fill
    assert_eq!(s.doc.section_planes.len(), 1);
    let cut = &s.doc.section_planes[0];
    assert_eq!(
        (cut.name.as_str(), cut.symbol.as_str(), cut.active),
        ("Cut A", "A", true)
    );
}

#[test]
fn test_the_new_keys_are_not_saved_again() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &new_keys(), None).unwrap();
    let tmp = TmpDir::new();
    let out = tmp.join("saved.xtrude");
    document::save_scene(&s, &out).unwrap();
    let p = payload(&out);
    assert!(p.get("kept_components").is_none());
    assert!(p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g.get("locked").is_none()));
    assert!(p["section_planes"][0].get("fill").is_none());
    assert_eq!(
        p["units"],
        serde_json::json!({"length": "mm", "precision": 1})
    );
    assert_eq!(p["protos"].as_array().unwrap().len(), 1);
}

#[test]
fn test_an_ingetrazo_0_5_8_format_3_document_opens() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &format3(), None).unwrap();
    assert_eq!(s.load_repairs, 0);
    // the counts IngeTrazo 0.5.8 reopens it with
    let loose = s.mesh();
    assert_eq!(counts(loose), [3733, 7332, 3601]);
    let names: Vec<&str> = s.doc.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Plate", "Peg 0", "Peg 20"]);
    let layers: Vec<&str> = s.doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(layers, ["Layer 0", "Guides"]);

    // the points numbered as the JSON lists them: the first edge's ends
    let first: Vec<_> = loose
        .vertex_ids()
        .into_iter()
        .take(2)
        .map(|v| loose.pos(v))
        .collect();
    assert!(near(first[0], v3(0., 0., 0.)) && near(first[1], v3(0.001, 0., 0.)));
    for p in [
        v3(0.06, 0.06, 0.),
        v3(0.03, 0.017, 0.),
        v3(0.015, 0.005, 0.01),
    ] {
        assert!(has_vertex(loose, p), "{p:?}");
    }
    let colours: Vec<[f64; 3]> = loose
        .faces
        .iter()
        .filter_map(|(_, f)| f.attrs.paint.color)
        .collect();
    assert_eq!(colours.len(), 61);
    assert!(colours[..60].iter().all(|c| *c == [0.2, 0.6, 0.3]));
    assert_eq!(colours[60], [0.9, 0.9, 0.1]);
    let on_guides = loose
        .faces
        .iter()
        .filter(|(_, f)| f.attrs.layer.as_deref() == Some("Guides"));
    assert_eq!(on_guides.count(), 1);
    let holed: Vec<_> = loose
        .faces
        .iter()
        .filter(|(_, f)| !f.holes.is_empty())
        .collect();
    assert_eq!(holed.len(), 1);
    assert_eq!(holed[0].1.holes.len(), 2);

    let plate = &s.doc.groups[0];
    assert!(!plate.is_component());
    assert_eq!(
        plate.material.as_ref().and_then(|p| p.color),
        Some([0.3, 0.3, 0.8])
    );
    let m = s.doc.mesh(plate.mesh);
    assert_eq!(counts(m), [2601, 5100, 2500]);
    assert!(has_vertex(m, v3(0.2, 0.1, 0.)));
    let edges: Vec<_> = m.edges.iter().map(|(_, e)| e).collect();
    assert_eq!(edges.iter().filter(|e| e.soft).count(), 100);
    assert_eq!(edges.iter().filter(|e| e.hidden).count(), 67);
    assert_eq!(edges.iter().filter(|e| e.curve == Some(41)).count(), 50);
    assert_eq!(
        edges
            .iter()
            .filter(|e| e.layer.as_deref() == Some("Guides"))
            .count(),
        20
    );

    let pegs = &s.doc.groups[1..];
    assert!(pegs
        .iter()
        .all(|g| g.is_component() && g.mesh == pegs[0].mesh));
    let peg = s.doc.mesh(pegs[0].mesh);
    assert_eq!(counts(peg), [676, 1300, 625]);
    assert!(has_vertex(peg, v3(0.01, 0.01, 0.)));
    let red = peg
        .faces
        .iter()
        .filter(|(_, f)| f.attrs.paint.color == Some([1.0, 0.0, 0.0]));
    assert_eq!(red.count(), 1);
    let at: Vec<_> = pegs
        .iter()
        .map(|g| g.xform.unwrap().w_axis.truncate())
        .collect();
    assert!(near(at[0], v3(0., -0.02, 0.)) && near(at[1], v3(0.02, -0.02, 0.)));
}

#[test]
fn test_a_format_3_document_saves_and_reopens_the_same() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &format3(), None).unwrap();
    let back = roundtrip(&s);
    assert_eq!(counts(back.mesh()), counts(s.mesh()));
    for (a, b) in s.doc.groups.iter().zip(&back.doc.groups) {
        assert_eq!(counts(back.doc.mesh(b.mesh)), counts(s.doc.mesh(a.mesh)));
    }
}
