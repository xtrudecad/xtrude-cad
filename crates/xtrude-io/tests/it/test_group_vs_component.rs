// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Groups and components through the document.

use crate::common;
use common::*;
use std::collections::HashMap;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::{document, igz};
use xtrude_model::commands::{MakeComponentOfCommand, MakeNestedGroupCommand};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

fn box_group(s: &mut Scene, x: f64, name: &str) -> Group {
    let mut m = Mesh::new();
    quad(&mut m, x, x + 1., 0., 1., 0.);
    Group::new(add_mesh(&mut s.doc.meshes, m), Some(name))
}

fn kinds(s: &Scene) -> HashMap<String, bool> {
    s.doc
        .groups
        .iter()
        .map(|g| (g.name.clone(), g.is_component()))
        .collect()
}

fn spacer(s: &mut Scene, h: &mut History) {
    let (a, b) = (box_group(s, 0., "A"), box_group(s, 3., "B"));
    let ids = vec![a.id, b.id];
    s.doc.groups.extend([a, b]);
    let mut cmd = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids, Some("Spacer"));
    h.execute(s, &mut cmd);
}

#[test]
fn test_the_flag_survives_the_document_and_old_files_read_as_groups() {
    let mut s = Scene::new();
    let mut h = History::new();
    spacer(&mut s, &mut h);
    let (c, d) = (box_group(&mut s, 6., "C"), box_group(&mut s, 9., "D"));
    let ids = vec![c.id, d.id];
    s.doc.groups.extend([c, d]);
    let nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids, Some("Clip"));
    h.execute(&mut s, &mut MakeComponentOfCommand { nest });
    let back = roundtrip(&s);
    assert_eq!(
        kinds(&back),
        HashMap::from([("Spacer".to_string(), false), ("Clip".to_string(), true)])
    );
    assert!(back.doc.groups.iter().all(|g| g.children.len() == 2));
}

fn strip(entry: &mut serde_json::Value) {
    if let Some(o) = entry.as_object_mut() {
        o.remove("component");
    }
    if let Some(kids) = entry.get_mut("children").and_then(|c| c.as_array_mut()) {
        kids.iter_mut().for_each(strip);
    }
}

#[test]
fn test_an_older_file_reads_its_groups_of_groups_as_groups() {
    let mut s = Scene::new();
    let mut h = History::new();
    spacer(&mut s, &mut h);
    let p = box_group(&mut s, 0., "P");
    let mut placed = Group::new(p.mesh, Some("Part"));
    placed.xform = Some(DMat4::IDENTITY);
    let mut imported = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("Import"));
    imported.adopt(vec![placed]);
    s.doc.groups.push(imported);
    let tmp = TmpDir::new();
    let new = tmp.join("new.xtr");
    document::save_scene(&s, &new).unwrap();
    // an IngeTrazo document from before the key existed
    let mut doc = igz_json(&s);
    for g in doc["scene"]["groups"].as_array_mut().unwrap() {
        strip(g);
    }
    let old = tmp.join("old.igz");
    std::fs::write(&old, doc.to_string()).unwrap();
    let mut back = Scene::new();
    igz::load_into(&mut back, &old, None).unwrap();
    assert_eq!(
        kinds(&back),
        HashMap::from([("Spacer".to_string(), false), ("Import".to_string(), true)])
    );
}
