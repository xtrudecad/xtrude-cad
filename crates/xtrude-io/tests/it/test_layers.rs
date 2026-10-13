// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Layers through the document.

use crate::common;
use common::*;
use xtrude_model::layers::{layer_name, Layer, DEFAULT_LAYER};
use xtrude_model::scene::Scene;

#[test]
fn test_layers_and_labels_round_trip_document() {
    let mut s = Scene::new();
    let f = quad(s.mesh_mut(), 0., 2., 0., 2., 0.);
    let e = s.mesh().edge_ids()[0];
    let mut shells = Layer::new("Shells");
    shells.visible = false;
    shells.locked = true;
    s.doc.layers.push(shells);
    s.mesh_mut().face_mut(f).attrs.layer = Some("Shells".into());
    s.mesh_mut().edge_mut(e).layer = Some("Shells".into());
    let s2 = roundtrip(&s);
    let names: std::collections::HashMap<&str, (bool, bool)> = s2
        .doc
        .layers
        .iter()
        .map(|l| (l.name.as_str(), (l.visible, l.locked)))
        .collect();
    assert_eq!(names[DEFAULT_LAYER], (true, false));
    assert_eq!(names["Shells"], (false, true));
    let m = s2.mesh();
    let f2 = m.face_ids()[0];
    assert_eq!(layer_name(m.face(f2).attrs.layer.as_deref()), "Shells");
    assert_eq!(
        m.edge_ids()
            .iter()
            .filter(|&&e| layer_name(m.edge(e).layer.as_deref()) == "Shells")
            .count(),
        1
    );
}
