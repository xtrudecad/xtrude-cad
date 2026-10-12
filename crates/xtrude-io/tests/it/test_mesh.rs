// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The mesh through .igz.

use crate::common;
use common::*;
use xtrude_io::igz;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

#[test]
fn test_mesh_is_read_compatible_with_igz_save() {
    let mut s = Scene::new();
    let m = s.doc.mesh_mut(MeshId::LOOSE);
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
        &[vec![
            v3(1., 1., 0.),
            v3(3., 1., 0.),
            v3(3., 3., 0.),
            v3(1., 3., 0.),
        ]],
    );
    m.add_edge(v3(0., 0., 0.), v3(0., 5., 0.)).unwrap();
    let tmp = TmpDir::new();
    let out = tmp.join("mesh.igz");
    igz::save_scene(&s, &out).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        data["scene"]["faces"][0]["vertices"][0],
        serde_json::json!([0.0, 0.0, 0.0])
    );
    assert_eq!(
        data["scene"]["faces"][0]["holes"][0]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(!data["scene"]["edges"].as_array().unwrap().is_empty());
}
