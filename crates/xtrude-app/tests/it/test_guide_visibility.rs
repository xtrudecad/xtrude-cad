// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A guide whose far end lands behind the camera is clipped to its visible
//! span, fed to the snap engine with projectable ends, and snaps in
//! perspective.

use crate::common;

use common::{window, MetreDocument};
use glam::DVec3;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::{compute_snap, SnapKind, SnapQuery};
use xtrude_model::guide::Guide;

/// A guide from the origin TOWARD the eye: its far end lands behind the
/// camera.
fn window_with_guide() -> (MainWindow, Guide) {
    let mut w = window();
    w.viewport.resize(1200., 800.);
    w.viewport.camera.set_aspect(1200, 800);
    let d = w.viewport.camera.eye().normalize();
    let g = Guide::new(DVec3::ZERO, Some(d), None);
    w.viewport.scene.doc.guides.push(g.clone());
    (w, g)
}

#[test]
fn test_clip_segment_front_recovers_projectable_span() {
    let (w, g) = window_with_guide();
    let vp = &w.viewport;
    let (a, b) = g.segment();
    assert!(vp.world_to_pixel(a).is_none() || vp.world_to_pixel(b).is_none()); // the bug's precondition
    let (ca, cb) = vp.clip_segment_front(a, b).expect("a span");
    assert!(vp.world_to_pixel(ca).is_some() && vp.world_to_pixel(cb).is_some());
}

#[test]
fn test_snap_scene_feeds_clipped_guides_and_points() {
    let (mut w, _g) = window_with_guide();
    let vp = &mut w.viewport;
    vp.scene
        .doc
        .guides
        .push(Guide::new(DVec3::new(2., 3., 0.), None, None)); // a point
    let edges = vp.snap_scene(Some((600., 400.)));
    let projectable = edges
        .iter()
        .filter(|e| vp.world_to_pixel(e.a).is_some() && vp.world_to_pixel(e.b).is_some())
        .count();
    assert!(projectable >= 2); // the clipped line and the point
    assert!(edges
        .iter()
        .any(|e| (e.a - DVec3::new(2., 3., 0.)).length() < 1e-9));
}

#[test]
fn test_on_guide_snap_in_perspective() {
    let _m = MetreDocument::new();
    let (mut w, g) = window_with_guide();
    let d = g.direction.unwrap();
    let p = g.point + d * 3.0; // a visible point on the guide
    let vp = &mut w.viewport;
    let px = vp.world_to_pixel(p).expect("on screen");
    let edges = vp.snap_scene(Some(px.tuple()));
    let vp = &w.viewport;
    let w2p = |q: DVec3| vp.world_to_pixel(q).map(|p| p.tuple());
    let mut q = SnapQuery::new(p, px.tuple(), &w2p, vp.prefs.snap_threshold_px);
    q.edge_threshold_px = Some(vp.prefs.edge_snap_threshold_px);
    let snap = compute_snap(&q, &edges);
    assert_ne!(snap.kind, SnapKind::None); // snappable again
    let off = snap.point - g.point;
    assert!((off - d * off.dot(d)).length() < 1e-3); // on the guide line
}
