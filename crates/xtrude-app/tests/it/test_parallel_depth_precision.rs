// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! In the parallel projection, the depth buffer tells apart surfaces a
//! fraction of a millimetre apart. Its range was ±10 km whatever the model,
//! which in `f32` left steps of about a millimetre: edges and holes behind
//! a thin plate showed through it, and the back edges of a tube. The frame
//! now fits the range to the scene.

use crate::common;

use common::{add_box, sized_viewport, v3};
use xtrude_app::viewport::Viewport;
use xtrude_core::DVec3;
use xtrude_render::frame::FrameData;

/// A part about half a metre across, framed in parallel.
fn part() -> Viewport {
    let mut vp = sized_viewport(1400.0, 900.0);
    add_box(&mut vp, v3(-0.1, 0.05, 0.0), v3(0.4, 0.35, 0.2));
    vp.camera.set_view("iso");
    vp.camera.perspective = false;
    let (lo, hi) = vp.scene.bounds().expect("a part");
    vp.camera.fit_box(lo, hi, 1.1);
    vp
}

/// The depth the GPU stores for `p`, computed in `f32` through the
/// shader's matrix, as the vertex shader does.
fn gpu_depth(f: &FrameData, p: DVec3) -> f32 {
    let m = f.shader_mvp();
    let l = f.local(p);
    let v = [l[0], l[1], l[2], 1.0f32];
    let row = |r: usize| (0..4).map(|c| m[c][r] * v[c]).sum::<f32>();
    row(2) / row(3)
}

#[test]
fn test_points_a_fifth_of_a_millimetre_apart_get_different_depths() {
    let mut vp = part();
    let f = vp.frame_data([1400, 900], false);
    let fwd = vp.camera.forward();
    // Across the part: every pair, 0.2 mm apart along the view, is told
    // apart.
    for i in 0..=10 {
        for j in 0..=10 {
            let p = v3(-0.1 + 0.05 * i as f64, 0.05 + 0.03 * j as f64, 0.1);
            let (near, far) = (gpu_depth(&f, p), gpu_depth(&f, p + fwd * 2.0e-4));
            assert!(far > near, "{p}: {near} and {far}");
        }
    }
}

#[test]
fn test_picking_uses_the_matrix_the_frame_was_drawn_with() {
    let mut vp = part();
    let f = vp.frame_data([1400, 900], false);
    assert_eq!(f.view_proj, vp.mvp());
    let (near, far) = vp.camera.depth_range();
    assert!(far - near < 100.0, "range {near}..{far}");
}

#[test]
fn test_the_range_follows_an_edit() {
    let mut vp = part();
    vp.frame_data([1400, 900], false);
    let before = vp.camera.depth_range();
    // A small box 40 m behind the part.
    let back = vp.camera.target + vp.camera.forward() * 40.0;
    add_box(&mut vp, back, back + v3(0.01, 0.01, 0.01));
    vp.frame_data([1400, 900], false);
    let after = vp.camera.depth_range();
    assert!(after.1 > before.1 + 30.0, "{before:?} → {after:?}");
    // The frame just drawn may still be read back for the zoom.
    assert!(vp.wants_depth());
}
