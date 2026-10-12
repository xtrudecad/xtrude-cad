// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport projects and picks through its `ViewProbe`: the camera in
//! the app, nothing in a stand-in, the test's answers in a `FakeProbe`.

use crate::common::{self, v3, FakeProbe};
use xtrude_app::input::ScreenPt;
use xtrude_model::ids::next_id;

#[test]
fn test_a_viewport_projects_through_its_camera() {
    let vp = common::viewport();
    let probe = vp.probe();
    assert!(probe.projects());
    assert!(probe.camera(&vp).is_some());
    let p = v3(1., 2., 0.5);
    let px = vp.world_to_pixel(p).unwrap();
    let (x, y) = common::screen(&vp, p);
    assert!((px.x - x).abs() < 1e-9 && (px.y - y).abs() < 1e-9);
    let (o, d) = vp.pixel_to_ray(px).unwrap();
    assert!((p - o).cross(d).length() < 1e-6); // the ray passes through p
    assert_eq!(vp.eye(), vp.camera.eye());
}

#[test]
fn test_a_stand_in_neither_projects_nor_picks_a_face() {
    let mut vp = common::stub();
    let probe = vp.probe();
    assert!(!probe.projects());
    assert!(probe.camera(&vp).is_none());
    assert!(probe.eye(&vp).is_none());
    assert!(vp.world_to_pixel(v3(1., 2., 0.)).is_none());
    assert!(vp.pixel_to_ray(ScreenPt::new(10., 10.)).is_none());
    assert!(vp.pick_face_any(10., 10.).is_none());
    // the eye falls back on the stand-in's unused camera
    assert_eq!(vp.eye(), vp.camera.eye());
    // a lock line without a ray keeps its start
    let start = v3(1., 1., 1.);
    assert_eq!(
        vp.project_to_lock_line(start, v3(1., 0., 0.), 5., 5.),
        start
    );
}

#[test]
fn test_a_fake_probe_answers_through_every_handle() {
    let mut vp = common::stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x * 10.0, p.y * 10.0)));
    let g = next_id();
    FakeProbe::of(&mut vp).pick_group(move |_, _| Some(g));
    assert!(vp.probe().projects());
    assert_eq!(
        vp.world_to_pixel(v3(1., 2., 0.)),
        Some(ScreenPt::new(10., 20.))
    );
    assert_eq!(vp.pick_group(0., 0.), Some(g));
    FakeProbe::of(&mut vp).eye(Some(v3(0., -5., 0.)));
    assert_eq!(vp.eye(), v3(0., -5., 0.));
}
