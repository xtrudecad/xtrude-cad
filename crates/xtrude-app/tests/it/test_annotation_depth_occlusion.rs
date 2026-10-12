// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The overlay's annotations learn what is hidden from the depth the frame
//! just rendered, not from ray casts. The frame is drawn headless through wgpu
//! and its depth read back as the shell does; skipped where the machine offers
//! no adapter.

use crate::common;

use common::{add_box, v3, window, Gpu};
use xtrude_model::dimension::Dimension;

#[test]
fn test_a_point_behind_a_wall_is_hidden_and_one_in_front_is_not() {
    let Some(mut gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize_gl(800.0, 600.0);
    add_box(vp, v3(-2., -0.1, 0.), v3(2., 0.1, 3.)); // a wall across Y = 0
    vp.scene.doc.dimensions.push(Dimension::new(
        v3(-1., 2., 1.),
        v3(1., 2., 1.),
        v3(0., 0., 0.5),
    ));
    vp.scene.version += 1;
    vp.camera.set_view("front"); // looking toward +Y
    vp.camera.perspective = false;
    vp.camera.fit_to(v3(-3., -1., 0.), v3(3., 3., 3.), 1.3);
    gpu.grab(vp); // a real frame, drawn the way the shell does
    if vp.depth_snap().is_none() {
        eprintln!("this adapter gives no depth read-back: skipped");
        return;
    }
    let behind = v3(0., 2., 1.); // the wall hides it
    let front = v3(0., -2., 1.); // nothing in front
    let on_face = v3(0., -0.1, 1.); // ON the wall's face
    assert!(vp.occluded_on_screen(behind));
    assert!(!vp.occluded_on_screen(front));
    assert!(!vp.occluded_on_screen(on_face));
    // the rays agree on all three
    assert!(vp.is_occluded(behind));
    assert!(!vp.is_occluded(front));
    assert!(!vp.is_occluded(on_face));
}
