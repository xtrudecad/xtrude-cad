// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The zoom and the pan's grabbed point read the point under the cursor
//! from the depth of the frame on screen, not from the pick index. The
//! frame is drawn headless through wgpu and its depth handed over as the shell does
//! before a wheel notch or a press; skipped where the machine offers no
//! adapter.

use crate::common;

use common::{add_box, sized_viewport, v3, Gpu};
use xtrude_app::viewport::Viewport;

/// The window: a box with its front face at y = −1, seen from the
/// front in parallel projection, one frame on screen.
fn win() -> Option<(Viewport, Gpu)> {
    let Some(mut gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return None;
    };
    let mut vp = sized_viewport(800.0, 600.0);
    vp.scene.guides.clear();
    add_box(&mut vp, v3(-1., -1., 0.), v3(1., 1., 2.));
    vp.camera.set_view("front"); // looking toward +Y
    vp.camera.perspective = false;
    vp.camera.fit_to(v3(-3., -1., -1.), v3(3., 1., 3.), 1.3);
    gpu.grab(&mut vp); // a frame on screen
    gpu.depth_for_input(&mut vp); // a wheel notch or a press reads its depth
    if vp.depth_world_at(10.0, 10.0).is_none() {
        eprintln!("this adapter gives no depth read-back: skipped");
        return None;
    }
    Some((vp, gpu))
}

fn approx(p: glam::DVec3, q: glam::DVec3) {
    assert!((p - q).abs().max_element() <= 0.05, "{p:?} vs {q:?}");
}

#[test]
fn test_the_zoom_finds_the_face_under_the_cursor_from_the_depth() {
    let Some((mut vp, _gpu)) = win() else { return };
    let built = vp.pick_index_builds();
    let px = vp.world_to_pixel(v3(0.3, -1.0, 1.2)).unwrap();
    let p = vp.world_under_cursor(px.x, px.y);
    approx(p.expect("a point"), v3(0.3, -1.0, 1.2));
    assert_eq!(vp.pick_index_builds(), built);
}

#[test]
fn test_the_pan_grab_point_too() {
    let Some((mut vp, _gpu)) = win() else { return };
    let built = vp.pick_index_builds();
    let px = vp.world_to_pixel(v3(-0.5, -1.0, 0.5)).unwrap();
    let p = vp.grab_point_at(px.x, px.y);
    approx(p, v3(-0.5, -1.0, 0.5));
    assert_eq!(vp.pick_index_builds(), built);
}

#[test]
fn test_the_sky_does_not_build_the_index_either() {
    let Some((mut vp, _gpu)) = win() else { return };
    let built = vp.pick_index_builds();
    let px = vp.world_to_pixel(v3(2.8, -1.0, 2.8)).unwrap(); // beside the box
    vp.world_under_cursor(px.x, px.y);
    vp.grab_point_at(px.x, px.y);
    assert_eq!(vp.pick_index_builds(), built);
}

#[test]
fn test_after_the_camera_moves_without_a_frame_it_falls_back() {
    let Some((mut vp, _gpu)) = win() else { return };
    let h = vp.height() as i32;
    vp.camera.orbit(30.0, 5.0, h); // no repaint yet
    assert!(vp.depth_world_at(10.0, 10.0).is_none());
}
