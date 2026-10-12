// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window tests: opening a document restores its author's view, New resets it
//! (a printer bed for a millimetre document), and a part without a saved
//! camera — or imported — opens framed. The .igz tests are in xtrude-io.

use crate::common;

use common::{v3, window, TmpDir};
use glam::DVec3;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::Mesh;
use xtrude_io::{dae, igz};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

#[test]
fn test_open_restores_the_authors_view_and_new_resets_it() {
    let dir = TmpDir::new();
    let mut w = window();
    w.mark_saved();
    {
        let c = &mut w.viewport.camera;
        c.target = v3(12., -4., 1.5);
        c.distance = 33.0;
        c.yaw = 1.1;
        c.pitch = 0.2;
        c.perspective = false;
    }
    let path = dir.path().join("doc.igz");
    w.do_save(&path);
    let home = w.viewport.scene.camera_home.clone().expect("a saved view");
    assert_eq!(home.distance, Some(33.0));
    // Move the camera away, then reopen: back where the author left it.
    {
        let c = &mut w.viewport.camera;
        c.target = DVec3::ZERO;
        c.distance = 5.0;
        c.yaw = -0.7;
        c.perspective = true;
    }
    w.mark_saved();
    assert!(w.open_path(&path));
    let c = &w.viewport.camera;
    assert!((c.target - v3(12., -4., 1.5)).length() < 1e-6);
    assert!(c.distance == 33.0 && (c.yaw - 1.1).abs() < 1e-9);
    assert!(!c.perspective);
    // New: the default view, framed on a millimetre document's printer bed.
    w.mark_saved();
    w.trigger("new");
    let c = &w.viewport.camera;
    assert!(0.1 < c.distance && c.distance < 1.0 && c.perspective);
    assert!((c.yaw - (-45f64).to_radians()).abs() < 1e-9);
    assert!((c.target - v3(0.125, 0.125, 0.0625)).length() < 1e-9);
}

#[test]
fn test_a_new_millimetre_document_opens_on_a_printer_bed() {
    let mut w = window();
    assert_eq!(w.viewport.scene.units.length, "mm");
    assert!(0.1 < w.viewport.camera.distance && w.viewport.camera.distance < 1.0); // not 20 m out
    w.viewport.camera.distance = 20.0;
    w.trigger("new");
    assert!(0.1 < w.viewport.camera.distance && w.viewport.camera.distance < 1.0);
}

/// A 20 × 10 × 5 mm block, as a printed part would be drawn.
fn small_part(s: &mut Scene) {
    let (a, b, c) = (0.020, 0.010, 0.005);
    let mut m = Mesh::new();
    for lp in [
        [v3(0., 0., 0.), v3(0., b, 0.), v3(a, b, 0.), v3(a, 0., 0.)],
        [v3(0., 0., c), v3(a, 0., c), v3(a, b, c), v3(0., b, c)],
        [v3(0., 0., 0.), v3(a, 0., 0.), v3(a, 0., c), v3(0., 0., c)],
        [v3(a, 0., 0.), v3(a, b, 0.), v3(a, b, c), v3(a, 0., c)],
        [v3(a, b, 0.), v3(0., b, 0.), v3(0., b, c), v3(a, b, c)],
        [v3(0., b, 0.), v3(0., 0., 0.), v3(0., 0., c), v3(0., b, c)],
    ] {
        m.add_face(&lp, &[]);
    }
    let g = Group::new(add_mesh(&mut s.doc.meshes, m), Some("Part"));
    s.doc.groups.push(g);
}

/// The camera looks at the part from closer than half a metre.
fn frames_the_part(c: &OrbitCamera) -> bool {
    c.distance < 0.5 && (c.target - v3(0.01, 0.005, 0.0025)).length() < 0.05
}

#[test]
fn test_a_document_without_a_saved_camera_opens_framed() {
    let dir = TmpDir::new();
    let mut s = Scene::new();
    small_part(&mut s);
    let path = dir.path().join("part.igz");
    igz::save_scene(&s, &path).unwrap(); // no camera_home
    let mut w = window();
    w.viewport.camera.distance = 20.0;
    assert!(w.open_path(&path));
    assert!(frames_the_part(&w.viewport.camera));
}

#[test]
fn test_an_imported_part_is_framed() {
    let dir = TmpDir::new();
    let mut s = Scene::new();
    small_part(&mut s);
    let path = dir.path().join("part.dae");
    dae::save_dae(&s, &path, "part").unwrap();
    let mut w = window();
    w.viewport.camera.distance = 20.0;
    w.import_dae_path(&path);
    let sc = &w.viewport.scene;
    assert!(!sc.mesh().face_ids().is_empty() || !sc.doc.groups.is_empty());
    assert!(frames_the_part(&w.viewport.camera));
}
