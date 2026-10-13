// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! OBJ import: units, foreign-encoded .mtl, smoothing groups.

use crate::common;
use common::*;
use xtrude_io::{document, obj};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

#[test]
fn test_the_document_is_gzipped_json() {
    let mut s = Scene::new();
    let mut h = History::new();
    drawn_cube(&mut s, &mut h);
    let tmp = TmpDir::new();
    let doc = tmp.join("cube.xtrude");
    document::save_scene(&s, &doc).unwrap();
    assert!(std::fs::read(&doc).unwrap().starts_with(b"\x1f\x8b"));
    let data = document_json(&doc);
    assert_eq!(data["xtrude_format"], 1);
    assert!(data.get("igz_format").is_none());
}

#[test]
fn test_an_mtl_in_a_foreign_encoding_still_loads() {
    let tmp = TmpDir::new();
    std::fs::write(
        tmp.join("m.mtl"),
        b"# color caf\xe9\nnewmtl skin\nKd 0.8 0.1 0.1\n",
    )
    .unwrap();
    let path = tmp.join("m.obj");
    std::fs::write(
        &path,
        "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl skin\nf 1 2 3\n",
    )
    .unwrap();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &path, None, 1.0, None).unwrap();
    let m = s.mesh();
    assert_eq!(
        m.face(m.face_ids()[0]).attrs.paint.color,
        Some([0.8, 0.1, 0.1])
    );
}

fn obj_of_size(tmp: &TmpDir, sub: &str, span: f64) -> std::path::PathBuf {
    let dir = tmp.join(sub);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("s.obj");
    std::fs::write(
        &p,
        format!("v 0 0 0\nv {span} 0 0\nv 0 {span} 0\nf 1 2 3\n"),
    )
    .unwrap();
    p
}

#[test]
fn test_suggest_unit_only_speaks_when_metres_are_impossible() {
    let tmp = TmpDir::new();
    assert_eq!(obj::suggest_unit(&obj_of_size(&tmp, "a", 234.0)), "cm");
    assert_eq!(obj::suggest_unit(&obj_of_size(&tmp, "b", 45000.0)), "mm");
    assert_eq!(obj::suggest_unit(&obj_of_size(&tmp, "c", 115.0)), "m");
    assert_eq!(obj::suggest_unit(&obj_of_size(&tmp, "d", 3.2)), "m");
}

fn xs_ys(s: &Scene) -> (f64, f64) {
    let m = s.mesh();
    let pts: Vec<_> = m
        .face_ids()
        .iter()
        .flat_map(|&f| m.face_positions(f))
        .collect();
    (
        pts.iter().map(|p| p.x).fold(f64::MIN, f64::max),
        pts.iter().map(|p| p.y).fold(f64::MIN, f64::max),
    )
}

#[test]
fn test_the_import_scale_lands_the_model_in_metres() {
    let tmp = TmpDir::new();
    let p = tmp.join("chair.obj");
    std::fs::write(&p, "v 0 0 0\nv 51 0 0\nv 0 80 0\nf 1 2 3\n").unwrap();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &p, None, 0.01, None).unwrap();
    let (x, y) = xs_ys(&s);
    assert!((x - 0.51).abs() < 1e-6 && (y - 0.80).abs() < 1e-6);
}

#[test]
fn test_the_default_scale_leaves_a_model_untouched() {
    let tmp = TmpDir::new();
    let p = tmp.join("m.obj");
    std::fs::write(&p, "v 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 3\n").unwrap();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &p, None, 1.0, None).unwrap();
    assert!((xs_ys(&s).0 - 2.0).abs() < 1e-9);
}

fn obj_with_smoothing(tmp: &TmpDir, smooth: &str) -> std::path::PathBuf {
    let p = tmp.join("s.obj");
    std::fs::write(&p, format!("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 2 1\nv 1 2 1\n{smooth}\nf 1 2 3 4\nf 4 3 6 5\n")).unwrap();
    p
}

fn shared_soft(s: &Scene) -> Vec<bool> {
    let m = s.mesh();
    m.edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).faces.len() == 2)
        .map(|e| m.edge(e).soft)
        .collect()
}

#[test]
fn test_a_file_that_says_the_surface_is_smooth_is_believed() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &obj_with_smoothing(&tmp, "s 1"), None, 1.0, None).unwrap();
    let shared = shared_soft(&s);
    assert!(!shared.is_empty() && shared.iter().all(|&x| x));
}

#[test]
fn test_a_file_that_says_nothing_keeps_its_line() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &obj_with_smoothing(&tmp, "s off"), None, 1.0, None).unwrap();
    let shared = shared_soft(&s);
    assert!(!shared.is_empty() && !shared.iter().any(|&x| x));
}

#[test]
fn test_the_smoothing_group_never_reaches_the_model() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    obj::load_obj(&mut s, &obj_with_smoothing(&tmp, "s 1"), None, 1.0, None).unwrap();
    let m = s.mesh();
    assert!(m
        .face_ids()
        .iter()
        .all(|&f| m.face(f).attrs.smooth.is_none()));
}
