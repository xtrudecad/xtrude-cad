// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The document camera through the document.

use crate::common;
use common::*;
use serde_json::json;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::document;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::saved_views::CameraHome;
use xtrude_model::scene::Scene;

#[test]
fn test_the_camera_travels_in_the_document() {
    let mut s = Scene::new();
    let cam = json!({"target": [1.0, 2.0, 3.0], "distance": 7.5, "yaw": 0.3,
                     "pitch": 0.4, "fov_deg": 50.0, "perspective": false});
    s.camera_home = cam.as_object().map(CameraHome::from_json);
    let tmp = TmpDir::new();
    let path = tmp.join("cam.xtr");
    document::save_scene(&s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    assert_eq!(back.camera_home, s.camera_home);
    s.clear();
    assert!(s.camera_home.is_none());
}

#[test]
fn test_the_loader_reports_progress() {
    let mut s = Scene::new();
    for i in 0..3 {
        let x = i as f64;
        let mut m = Mesh::new();
        m.add_face(
            &[
                v3(x, 0., 0.),
                v3(x + 1., 0., 0.),
                v3(x + 1., 1., 0.),
                v3(x, 1., 0.),
            ],
            &[],
        );
        let mut g = Group::new(add_mesh(&mut s.doc.meshes, m), Some(&format!("g{i}")));
        g.xform = Some(DMat4::IDENTITY);
        s.doc.groups.push(g);
    }
    let tmp = TmpDir::new();
    let path = tmp.join("groups.xtr");
    document::save_scene(&s, &path).unwrap();
    let mut seen: Vec<(f64, String)> = Vec::new();
    let mut back = Scene::new();
    let mut cb = |f: f64, t: &str| seen.push((f, t.to_string()));
    document::load_into(&mut back, &path, Some(&mut cb)).unwrap();
    let fracs: Vec<f64> = seen.iter().map(|(f, _)| *f).collect();
    let mut sorted = fracs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(fracs == sorted && fracs[0] < 0.1 && *fracs.last().unwrap() >= 0.95);
    assert!(seen
        .iter()
        .any(|(_, t)| t.to_lowercase().contains("groups")));
    assert_eq!(back.doc.groups.len(), 3);
    document::load_into(&mut Scene::new(), &path, None).unwrap();
}
