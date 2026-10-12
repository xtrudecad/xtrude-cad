// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The preview hides a recess's base when the push goes AWAY from the eye —
//! whichever side of the face the camera is on — and Ctrl (keep the base)
//! keeps it.

use crate::common;

use common::{sized_viewport, v3};
use glam::DVec3;
use std::f64::consts::FRAC_PI_2;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::viewport::Viewport;
use xtrude_core::FaceId;
use xtrude_model::ids::MeshId;

/// A panel in the XZ plane with an attached notch, both facing +Y.
fn panel_with_notch(vp: &mut Viewport) -> FaceId {
    let outer = [
        v3(0., 0., 0.),
        v3(0., 0., 3.),
        v3(6., 0., 3.),
        v3(6., 0., 0.),
    ];
    let notch = [
        v3(2., 0., 0.),
        v3(2., 0., 2.),
        v3(3., 0., 2.),
        v3(3., 0., 0.),
    ];
    let rev: Vec<DVec3> = notch.iter().rev().copied().collect();
    let m = vp.scene.mesh_mut();
    let panel = m.add_face(&outer, &[rev]);
    let d = m.add_face(&notch, &[]);
    assert!(m.face_normal(panel).y > 0.99 && m.face_normal(d).y > 0.99);
    d
}

/// The camera 10 m off the wall on the side `side` (+1 front, −1 back).
fn look_from(vp: &mut Viewport, side: f64) {
    let c = &mut vp.camera;
    c.target = v3(3., 0., 1.);
    c.distance = 10.0;
    c.pitch = 0.0;
    for yaw in [0.0, FRAC_PI_2, -FRAC_PI_2, std::f64::consts::PI] {
        c.yaw = yaw;
        if (c.eye().y - 10.0 * side).abs() < 1e-6 {
            return;
        }
    }
    panic!("no yaw puts the eye there");
}

fn start(vp: &mut Viewport, notch: FaceId) -> PushPullTool {
    let mut pp = PushPullTool::new();
    pp.begin(vp, MeshId::LOOSE, notch);
    assert!(pp.pp.attached);
    pp
}

fn preview(vp: &mut Viewport, pp: &mut PushPullTool, e: f64) -> bool {
    pp.pp.extrusion = e;
    pp.show_light_preview(vp);
    let base = (MeshId::LOOSE, pp.pp.base_face.unwrap());
    vp.suppressed_faces().contains(&base)
}

#[test]
fn test_pushing_away_from_the_eye_hides_the_base_from_either_side() {
    // The FRONT toward the camera: normal +Y, eye at +Y.
    let mut vp = sized_viewport(800., 600.);
    let notch = panel_with_notch(&mut vp);
    look_from(&mut vp, 1.0);
    let mut pp = start(&mut vp, notch);
    assert!(preview(&mut vp, &mut pp, -0.5)); // into the wall, away from the eye
    assert!(!preview(&mut vp, &mut pp, 0.5)); // toward the eye: the base is covered

    // The BACK toward the camera: an inverted face, normal +Y, eye at -Y.
    let mut vp = sized_viewport(800., 600.);
    let notch = panel_with_notch(&mut vp);
    look_from(&mut vp, -1.0);
    let mut pp = start(&mut vp, notch);
    assert!(preview(&mut vp, &mut pp, 0.5)); // away from the eye: hide the base
    assert!(!preview(&mut vp, &mut pp, -0.5)); // toward the eye
}

#[test]
fn test_ctrl_keeps_the_base_whatever_the_camera_says() {
    let mut vp = sized_viewport(800., 600.);
    let notch = panel_with_notch(&mut vp);
    look_from(&mut vp, -1.0);
    let mut pp = start(&mut vp, notch);
    pp.pp.keep_base = true;
    assert!(!preview(&mut vp, &mut pp, 0.5));
}
