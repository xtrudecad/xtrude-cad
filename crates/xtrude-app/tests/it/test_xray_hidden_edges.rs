// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! X-ray fades the edges that sit behind a face, so looking down into a box
//! you can tell whether it has a lid. The frame is drawn headless through
//! wgpu; skipped where the machine offers no adapter.

use crate::common;

use common::{contrast, v3, viewport, Gpu};
use glam::DVec3;
use xtrude_model::style::style_by_name;

#[test]
fn test_an_edge_under_a_face_is_fainter_than_one_in_the_open() {
    let Some(mut gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    let mut vp = viewport();
    // A lid over the left half (x < 0), two metres up; nothing on the
    // right. Seen from above.
    vp.scene.mesh_mut().add_face(
        &[
            v3(-60., -60., 2.),
            v3(0., -60., 2.),
            v3(0., 60., 2.),
            v3(-60., 60., 2.),
        ],
        &[],
    );
    vp.scene.version += 1;
    let cam = &mut vp.camera;
    cam.target = DVec3::ZERO;
    (cam.distance, cam.yaw, cam.pitch) = (30.0, 0.0, 1.5);
    cam.perspective = false;
    vp.preview.style_override = style_by_name("X-ray");
    assert!(vp.preview.style_override.is_some());
    let bare = gpu.render_image(&mut vp, 300, 300);

    let under = vp
        .scene
        .mesh_mut()
        .add_edge(v3(-5., -4., 0.), v3(-5., 4., 0.))
        .unwrap();
    vp.scene.version += 1;
    let hidden = contrast(&bare, &gpu.render_image(&mut vp, 300, 300));
    vp.scene.mesh_mut().remove_edge(under);

    vp.scene
        .mesh_mut()
        .add_edge(v3(5., -4., 0.), v3(5., 4., 0.))
        .unwrap();
    vp.scene.version += 1;
    let shown = contrast(&bare, &gpu.render_image(&mut vp, 300, 300));

    assert!(hidden > 0, "X-ray must still show the edge through the lid");
    assert!(
        (hidden as f64) < 0.7 * shown as f64,
        "the edge under the lid must read fainter (hidden={hidden}, open={shown})"
    );
}
