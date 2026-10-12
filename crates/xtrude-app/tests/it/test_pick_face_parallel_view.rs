// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The face in front wins in both cameras (the coplanar tolerance is measured
//! from the eye, not along a parallel ray that starts 10 km back), and truly
//! coplanar faces still tie-break by size.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{sized_viewport, v3};
use std::f64::consts::FRAC_PI_2;
use xtrude_app::viewport::Viewport;
use xtrude_model::ids::MeshId;

fn front_view(vp: &mut Viewport) {
    let c = &mut vp.camera;
    c.target = v3(2., 1., 1.5);
    c.distance = 8.0;
    c.yaw = -FRAC_PI_2; // a front view, along +y
    c.pitch = 0.0;
}

#[test]
fn test_the_face_in_front_wins_in_both_cameras() {
    for perspective in [true, false] {
        let mut vp = sized_viewport(1000., 700.);
        // A big face facing the camera and a SMALLER one 30 cm behind it.
        let m = vp.scene.mesh_mut();
        let front = m.add_face(
            &[
                v3(0., 0., 0.),
                v3(4., 0., 0.),
                v3(4., 0., 3.),
                v3(0., 0., 3.),
            ],
            &[],
        );
        m.add_face(
            &[
                v3(1., 0.3, 1.),
                v3(3., 0.3, 1.),
                v3(3., 0.3, 2.),
                v3(1., 0.3, 2.),
            ],
            &[],
        );
        vp.scene.version += 1;
        front_view(&mut vp);
        vp.camera.perspective = perspective;
        let ScreenPt { x, y } = vp.world_to_pixel(v3(2., 0., 1.5)).unwrap();
        let hit = vp.pick_face_any(x, y).expect("a face");
        assert!(
            hit.face == front && hit.owner.is_none(),
            "perspective = {perspective}"
        );
        assert_eq!(vp.pick_face(x, y), Some((MeshId::LOOSE, front)));
    }
}

#[test]
fn test_truly_coplanar_faces_still_tie_break_by_size() {
    let mut vp = sized_viewport(1000., 700.);
    let m = vp.scene.mesh_mut();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 0., 3.),
            v3(0., 0., 3.),
        ],
        &[],
    );
    let small = m.add_face(
        &[
            v3(1., 0., 1.),
            v3(3., 0., 1.),
            v3(3., 0., 2.),
            v3(1., 0., 2.),
        ],
        &[],
    );
    vp.scene.version += 1;
    front_view(&mut vp);
    vp.camera.perspective = false;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(2., 0., 1.5)).unwrap();
    assert_eq!(vp.pick_face(x, y), Some((MeshId::LOOSE, small)));
    assert_eq!(vp.pick_face_any(x, y).map(|h| h.face), Some(small));
}
