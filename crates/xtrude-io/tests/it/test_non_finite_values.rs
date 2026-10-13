// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Document tests: (The value box tests live with the app.)

use crate::common;
use common::*;
use xtrude_io::document::{self, DocError};
use xtrude_model::scene::Scene;

#[test]
fn test_a_nan_is_never_saved_and_the_old_file_survives() {
    let tmp = TmpDir::new();
    let path = tmp.join("grille.xtr");
    let mut s = Scene::new();
    s.mesh_mut()
        .add_face(&[v3(0., 0., 0.), v3(1., 0., 0.), v3(1., 1., 0.)], &[]);
    document::save_scene(&s, &path).unwrap();
    let good = std::fs::read(&path).unwrap();
    let v = s.mesh().verts.ids()[0];
    s.mesh_mut().verts[v].position.x = f64::NAN;
    match document::save_scene(&s, &path) {
        Err(DocError::NonFinite { where_ }) => assert!(where_.contains("scene"), "{where_}"),
        other => panic!("expected NonFinite, got {other:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), good);
    assert!(!tmp.join("grille.xtr.part").exists());
}

#[test]
fn test_a_clean_document_is_strict_json() {
    let tmp = TmpDir::new();
    let path = tmp.join("ok.xtr");
    let mut s = Scene::new();
    s.mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(2., 0., 0.))
        .unwrap();
    document::save_scene(&s, &path).unwrap();
    let text = document_text(&path);
    assert!(!text.contains("NaN") && !text.contains("Infinity"));
    let _: serde_json::Value = serde_json::from_str(&text).unwrap();
}
