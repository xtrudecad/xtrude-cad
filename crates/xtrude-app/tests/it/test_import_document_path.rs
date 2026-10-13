// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Insert a document as a component without the file dialog — placed at a point
//! in one undo step, or waiting for the click; an unreadable file changes
//! nothing.

use crate::common;

use common::{v3, window, TmpDir};
use glam::DVec3;
use std::path::PathBuf;
use xtrude_app::tools::place_group::PlaceGroupTool;
use xtrude_app::window::MainWindow;
use xtrude_io::document;
use xtrude_model::group::world_mesh;
use xtrude_model::ids::GroupId;
use xtrude_model::scene::Scene;

/// A 1 × 1 × 0.5 box drawn from the origin, saved as block.xtr.
fn block(dir: &TmpDir) -> PathBuf {
    let mut s = Scene::new();
    let p = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
        v3(0., 0., 0.5),
        v3(1., 0., 0.5),
        v3(1., 1., 0.5),
        v3(0., 1., 0.5),
    ];
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        s.mesh_mut().add_face(&q.map(|i| p[i]), &[]);
    }
    let path = dir.path().join("block.xtr");
    document::save_scene(&s, &path).unwrap();
    path
}

fn bounds(w: &MainWindow, g: GroupId) -> (DVec3, DVec3) {
    let doc = &w.viewport.scene.doc;
    let m = world_mesh(&doc.meshes, doc.find_group(g).unwrap());
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for (_, v) in m.verts.iter() {
        lo = lo.min(v.position);
        hi = hi.max(v.position);
    }
    (lo, hi)
}

fn has(w: &MainWindow, g: GroupId) -> bool {
    w.viewport.scene.doc.groups.iter().any(|x| x.id == g)
}

#[test]
fn test_at_a_point_the_component_lands_there_in_one_undo_step() {
    let dir = TmpDir::new();
    let mut w = window();
    let n0 = w.viewport.scene.doc.groups.len();
    let comp = w
        .import_document_path(&block(&dir), Some(v3(4., 2., 0.)))
        .unwrap()
        .expect("a component");
    assert!(has(&w, comp) && w.viewport.scene.doc.groups.len() == n0 + 1);
    let (lo, hi) = bounds(&w, comp); // its origin went to (4, 2, 0)
    assert!((lo - v3(4., 2., 0.)).length() < 1e-6, "{lo}");
    assert!((hi - v3(5., 3., 0.5)).length() < 1e-6, "{hi}");
    w.viewport.undo();
    assert!(!has(&w, comp) && w.viewport.scene.doc.groups.len() == n0);
}

#[test]
fn test_without_a_point_it_waits_for_the_click() {
    let dir = TmpDir::new();
    let mut w = window();
    let n0 = w.viewport.scene.doc.groups.len();
    let comp = w.import_document_path(&block(&dir), None).unwrap();
    assert!(w.viewport.tool::<PlaceGroupTool>().is_some());
    assert!(comp.is_none_or(|c| !has(&w, c)));
    assert_eq!(w.viewport.scene.doc.groups.len(), n0);
}

#[test]
fn test_an_unreadable_file_raises_and_changes_nothing() {
    let dir = TmpDir::new();
    let bad = dir.path().join("broken.xtr");
    std::fs::write(&bad, b"not a document").unwrap();
    let mut w = window();
    let n0 = w.viewport.scene.doc.groups.len();
    assert!(w.import_document_path(&bad, Some(DVec3::ZERO)).is_err());
    assert_eq!(w.viewport.scene.doc.groups.len(), n0);
}
