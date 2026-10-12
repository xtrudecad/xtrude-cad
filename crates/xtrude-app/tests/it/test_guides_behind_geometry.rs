// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The pixel half: a guide behind a wall stays behind it. The
//! frame is drawn headless through wgpu; skipped where the machine offers no
//! adapter. The dash generator half is in xtrude-render's test file of the
//! same name.

use crate::common;

use common::{changed, v3, viewport, Gpu};
use glam::DVec3;
use xtrude_app::viewport::Viewport;
use xtrude_model::guide::Guide;
use xtrude_model::style::style_by_name;

/// A wall in the XZ plane at y = 0, seen from the eye at y = +20: y > 0 is
/// IN FRONT of the wall and y < 0 behind it. The wall is wider than the
/// view, so no end of a guide pokes past its edges.
fn scene_looking_down_minus_y() -> Option<(Viewport, Gpu)> {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return None;
    };
    let mut vp = viewport();
    vp.scene.mesh_mut().add_face(
        &[
            v3(-60., 0., -60.),
            v3(60., 0., -60.),
            v3(60., 0., 60.),
            v3(-60., 0., 60.),
        ],
        &[],
    );
    vp.scene.version += 1;
    let cam = &mut vp.camera;
    cam.target = DVec3::ZERO;
    (cam.distance, cam.yaw, cam.pitch) = (20.0, std::f64::consts::FRAC_PI_2, 0.0);
    cam.perspective = false;
    Some((vp, gpu))
}

fn guide_along_x(y: f64) -> Guide {
    Guide::new(v3(0., y, 0.), Some(DVec3::X), None)
}

#[test]
fn test_a_wall_hides_the_guide_behind_it() {
    let Some((mut vp, mut gpu)) = scene_looking_down_minus_y() else {
        return;
    };
    assert!(vp.camera.eye().y > 0.0); // the eye really is on +Y
    let bare = gpu.render_image(&mut vp, 300, 300);

    vp.scene.guides = vec![guide_along_x(4.0)]; // in front
    let shown = changed(&bare, &gpu.render_image(&mut vp, 300, 300));

    vp.scene.guides = vec![guide_along_x(-4.0)]; // behind
    let hidden = changed(&bare, &gpu.render_image(&mut vp, 300, 300));

    assert!(shown > 50, "a guide in front of the wall must be drawn");
    assert_eq!(
        hidden, 0,
        "the wall must hide the guide behind it (hidden={hidden}px, shown={shown}px)"
    );
}

#[test]
fn test_a_sheet_render_still_carries_no_guides() {
    // A styled render never drew the scaffolding.
    let Some((mut vp, mut gpu)) = scene_looking_down_minus_y() else {
        return;
    };
    vp.preview.style_override = style_by_name("Hidden line");
    assert!(vp.preview.style_override.is_some());
    let bare = gpu.render_image(&mut vp, 300, 300);
    vp.scene.guides = vec![guide_along_x(4.0)];
    let after = gpu.render_image(&mut vp, 300, 300);
    vp.preview.style_override = None;
    assert_eq!(changed(&bare, &after), 0);
}
