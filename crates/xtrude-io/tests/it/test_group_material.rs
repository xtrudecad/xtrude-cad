// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Group materials through the document.

use crate::common;
use common::*;
use xtrude_core::attrs::Paint;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

#[test]
fn test_the_group_material_survives_the_document() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    quad(&mut m, 0., 1., 0., 1., 0.);
    let blue = quad(&mut m, 2., 3., 0., 1., 0.);
    m.face_mut(blue).attrs.paint.color = Some([0., 0., 1.]);
    let mut g = Group::new(add_mesh(&mut s.doc.meshes, m), Some("box"));
    let red = Paint {
        color: Some([1., 0., 0.]),
        ..Default::default()
    };
    g.material = Some(red.clone());
    s.doc.groups.push(g);
    let back = roundtrip(&s);
    assert_eq!(back.doc.groups[0].material, Some(red));
}
