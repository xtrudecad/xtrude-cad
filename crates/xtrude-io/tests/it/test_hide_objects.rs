// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hidden objects through the document.

use crate::common;
use common::*;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::Scene;

fn boxm(m: &mut Mesh, x0: f64) {
    let p = |i: usize| v3(x0 + (i & 1) as f64, ((i >> 1) & 1) as f64, (i >> 2) as f64);
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        let pts: Vec<DVec3> = q.iter().map(|&i| p(i)).collect();
        m.add_face(&pts, &[]);
    }
}

#[test]
fn test_a_hidden_face_round_trips_through_document() {
    let mut s = Scene::new();
    boxm(s.mesh_mut(), 0.0);
    let f = s.mesh().face_ids()[2];
    s.mesh_mut().face_mut(f).attrs.hidden = true;
    let s2 = roundtrip(&s);
    let m = s2.mesh();
    assert_eq!(
        m.face_ids()
            .iter()
            .filter(|&&f| m.face(f).attrs.hidden)
            .count(),
        1
    );
}

#[test]
fn test_hidden_view_flags_travel_in_the_document_and_in_scenes() {
    let mut s = Scene::new();
    s.show_hidden_objects = true;
    let s2 = roundtrip(&s);
    assert!(s2.show_hidden_objects && !s2.show_hidden_geometry);

    let mut cam = OrbitCamera::new();
    cam.target = DVec3::ZERO;
    cam.distance = 10.0;
    cam.yaw = 0.5;
    cam.pitch = 0.4;
    let view = SavedView::capture("ghosts", &s, &cam);
    let hs = view.hidden_shown.clone().unwrap();
    assert_eq!(hs.get("objects"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(hs.get("geometry"), Some(&serde_json::Value::Bool(false)));
    s.show_hidden_objects = false;
    SavedView::from_json(&view.to_json()).apply(&mut s, &mut cam);
    assert!(s.show_hidden_objects);
}

#[test]
fn test_hidden_flag_and_uid_round_trip_through_document() {
    let mut s = Scene::new();
    let mut ma = Mesh::new();
    boxm(&mut ma, 0.0);
    let mut mb = Mesh::new();
    boxm(&mut mb, 3.0);
    let g1 = Group::new(add_mesh(&mut s.doc.meshes, ma), Some("a"));
    let mut g2 = Group::new(add_mesh(&mut s.doc.meshes, mb), Some("b"));
    g2.hidden = true;
    let (u1, u2) = (g1.uid.clone(), g2.uid.clone());
    s.doc.groups.extend([g1, g2]);
    let s2 = roundtrip(&s);
    let a = s2.doc.groups.iter().find(|g| g.name == "a").unwrap();
    let b = s2.doc.groups.iter().find(|g| g.name == "b").unwrap();
    assert!(!a.hidden && b.hidden);
    assert!(a.uid == u1 && b.uid == u2);
}
