// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Saved views: the document half (the document round trip lives in xtrude-io).

use xtrude_core::camera::OrbitCamera;
use xtrude_core::DVec3;
use xtrude_model::layers::Layer;
use xtrude_model::saved_views::{from_lookat, LookAt, SavedView};
use xtrude_model::scene::Scene;

#[test]
fn test_capture_apply_round_trips_camera_and_layers() {
    let mut s = Scene::new();
    s.doc.layers.push(Layer::new("Shells"));
    let mut curves = Layer::new("Curves");
    curves.visible = false;
    s.doc.layers.push(curves);
    let mut cam = OrbitCamera::new();
    cam.target = DVec3::new(3.0, -2.0, 1.5);
    cam.distance = 42.0;
    cam.yaw = 1.234;
    cam.pitch = -0.4;
    cam.fov_deg = 30.0;
    cam.perspective = false;
    let view = SavedView::capture("Top", &s, &cam);
    assert_eq!(view.hidden_layers, ["Curves"]);
    let mut cam2 = OrbitCamera::new();
    for l in s.doc.layers.iter_mut() {
        l.visible = true;
    }
    view.apply(&mut s, &mut cam2);
    assert_eq!(cam2.target, DVec3::new(3.0, -2.0, 1.5));
    assert_eq!(cam2.distance, 42.0);
    assert_eq!(cam2.yaw, 1.234);
    assert_eq!(cam2.pitch, -0.4);
    assert_eq!(cam2.fov_deg, 30.0);
    assert!(!cam2.perspective);
    assert!(!s.doc.layer("Curves").unwrap().visible);
    assert!(s.doc.layer("Shells").unwrap().visible);
}

#[test]
fn test_apply_shows_layers_created_after_the_view_was_saved() {
    let mut s = Scene::new();
    let mut cam = OrbitCamera::new();
    let view = SavedView::capture("V", &s, &cam);
    let mut fresh = Layer::new("New");
    fresh.visible = false;
    s.doc.layers.push(fresh);
    view.apply(&mut s, &mut cam);
    assert!(s.doc.layer("New").unwrap().visible);
}

#[test]
fn test_from_lookat_oblique_view() {
    let v = from_lookat(
        "V",
        LookAt {
            eye: [10.0, 0.0, 10.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov_deg: 35.0,
            perspective: true,
            ortho_height: None,
        },
        vec![],
    );
    assert!((v.pose.distance - 200f64.sqrt()).abs() < 1e-9);
    assert!((v.pose.pitch.to_degrees() - 45.0).abs() < 1e-9);
    assert!(v.pose.yaw.to_degrees().abs() < 1e-9);
}

#[test]
fn test_from_lookat_plan_view_recovers_rotation_from_up() {
    let v = from_lookat(
        "Top",
        LookAt {
            eye: [5.0, 5.0, 100.0],
            target: [5.0, 5.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_deg: 45.0,
            perspective: true,
            ortho_height: None,
        },
        vec![],
    );
    assert!((v.pose.yaw.to_degrees() + 90.0).abs() < 1e-9);
    assert!((v.pose.pitch.to_degrees() - 89.0).abs() < 1e-9);
}

#[test]
fn test_from_lookat_parallel_frames_by_ortho_height() {
    let v = from_lookat(
        "P",
        LookAt {
            eye: [0.0, 0.0, 100.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_deg: 35.0,
            perspective: false,
            ortho_height: Some(32.6),
        },
        vec![],
    );
    let shown = 2.0 * v.pose.distance * (v.pose.fov_deg.to_radians() / 2.0).tan();
    assert!((shown - 32.6).abs() < 1e-9);
    assert!(!v.pose.perspective);
}

#[test]
fn test_scene_clear_drops_saved_views() {
    let mut s = Scene::new();
    s.saved_views.push(SavedView::new("V", Default::default()));
    s.clear();
    assert!(s.saved_views.is_empty());
}
