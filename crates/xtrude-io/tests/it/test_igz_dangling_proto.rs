// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A damaged .igz whose instances name a prototype that is not there opens
//! instead of panicking: each such group gets a mesh of its own.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_io::igz;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

#[test]
fn test_an_instance_with_a_missing_prototype_opens_with_its_own_mesh() {
    let tmp = TmpDir::new();
    let path = tmp.join("dangling.igz");
    let mut s = Scene::new();
    let mut m = xtrude_core::Mesh::new();
    quad(&mut m, 0., 1., 0., 1., 0.);
    let proto = add_mesh(&mut s.doc.meshes, m);
    for x in [0.0, 3.0] {
        let mut g = Group::new(proto, Some("copy"));
        g.xform = Some(DMat4::from_translation(v3(x, 0., 0.)));
        s.doc.groups.push(g);
    }
    let mut doc = igz_json(&s);
    let groups = doc["scene"]["groups"].as_array_mut().expect("groups");
    assert_eq!(groups.len(), 2);
    for g in groups {
        assert!(g.get("proto").is_some(), "an instance names its prototype");
        g["proto"] = serde_json::json!(99);
    }
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

    let mut back = Scene::new();
    igz::load_into(&mut back, &path, None).expect("opens");
    assert_eq!(back.doc.groups.len(), 2);
    let meshes: Vec<_> = back.doc.groups.iter().map(|g| g.mesh).collect();
    assert_ne!(meshes[0], meshes[1], "no shared prototype to fall back on");
    for g in &back.doc.groups {
        assert!(back.doc.mesh(g.mesh).faces.is_empty());
    }
}
