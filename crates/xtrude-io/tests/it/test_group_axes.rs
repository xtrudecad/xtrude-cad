// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Group axes through the document and .skp.

use crate::common;
use common::*;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

#[test]
fn test_the_axes_survive_the_document() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    quad(&mut m, 0., 2., 0., 1., 0.);
    let mut g = Group::new(add_mesh(&mut s.doc.meshes, m), Some("slab"));
    let turn = DMat4::from_translation(v3(1., 2., 3.)) * rotation_deg(DVec3::Z, 30.0);
    g.axes = Some(turn);
    s.doc.groups.push(g);
    let back = roundtrip(&s);
    assert_eq!(back.doc.groups[0].axes, Some(turn));
}

/// The groups the `.skp` import flattens into world coordinates (tagged
/// subtrees, containers of shared children) still face the way their
/// `.skp` instance did.
#[test]
fn test_a_flattened_skp_group_keeps_its_instance_axes() {
    use xtrude_core::math::MatExt;
    use xtrude_io::skp::{self, PayloadFace, PayloadGroup, SkpPayload};
    // A frame at (3, 0) turned 30°: translate, then turn about Z.
    let turn = DMat4::from_translation(v3(3., 0., 0.)) * rotation_deg(DVec3::Z, 30.0);
    let square = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
    ];
    let mut payload = SkpPayload::default();
    payload.groups.push(PayloadGroup {
        name: Some("turned".into()),
        faces: vec![PayloadFace {
            outer: square.iter().map(|p| turn.project_point(*p)).collect(),
            holes: Vec::new(),
            attrs: None,
        }],
        soft_edges: Some(Vec::new()),
        layer: None,
        axes: Some(turn),
    });
    let mut scene = Scene::new();
    skp::apply_payload(&mut scene, &payload).unwrap();
    let g = &scene.doc.groups[0];
    assert_eq!(g.axes, Some(turn));
    assert_eq!(g.xform, None);
}
