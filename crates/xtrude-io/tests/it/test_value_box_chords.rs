// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A damaged .igz with a NaN corner opens without it.

use crate::common;
use common::*;
use xtrude_io::igz;
use xtrude_model::scene::Scene;

#[test]
fn test_a_document_with_a_nan_corner_opens_without_it() {
    let tmp = TmpDir::new();
    let path = tmp.join("grille.igz");
    let mut good = Scene::new();
    quad(good.mesh_mut(), 0., 1., 0., 1., 0.);
    good.mesh_mut()
        .add_face(&[v3(3., 0., 0.), v3(4., 0., 0.), v3(4., 1., 0.)], &[]);
    igz::save_scene(&good, &path).unwrap();
    // Damage it the way it happened: one corner and one guide become NaN,
    // written as the bare word NaN.
    let mut data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    data["scene"]["faces"][1]["vertices"][0][0] = serde_json::json!("@NAN@");
    data["scene"]["guides"] = serde_json::json!([{"point": ["@NAN@", "@NAN@", "@NAN@"], "direction": null, "origin": null}]);
    let text = data.to_string().replace("\"@NAN@\"", "NaN");
    std::fs::write(&path, text).unwrap();
    let mut s = Scene::new();
    igz::load_into(&mut s, &path, None).unwrap();
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(s.mesh().verts.iter().all(|(_, v)| v.position.is_finite()));
    assert!(s.doc.guides.is_empty());
    assert!(s.load_repairs >= 2);
}
