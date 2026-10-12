// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport tests — the Center inference: hovering a curve's edge or a face
//! it bounds leaves the centre as a reference (a green dot) the cursor snaps
//! to; it goes with the circle, follows it, and a circle tiny on screen offers
//! none. (The engine half is in xtrude-core.)

use crate::common;
use crate::common::FakeProbe;

use glam::DVec3;
use xtrude_app::viewport::{CenterKey, FaceHit, Viewport};
use xtrude_core::snap::{compute_snap, SnapKind, SnapQuery};
use xtrude_model::commands::EraseSelectionCommand;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn circle_pts(cx: f64, cy: f64, r: f64, n: usize) -> Vec<DVec3> {
    (0..n)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            v(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// A stand-in viewport with a plan projection (x, y) → 10 px/m.
fn plan_view() -> Viewport {
    let mut vp = common::stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x * 10.0, p.y * 10.0)));
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    vp
}

/// A viewport over a tagged circle face centred (3, 2), r = 1.5; the face
/// is what the pick answers.
fn circle_view() -> (Viewport, xtrude_core::FaceId) {
    let mut vp = plan_view();
    let pts = circle_pts(3.0, 2.0, 1.5, 24);
    let f = vp.scene.mesh_mut().add_face(&pts, &[]);
    vp.scene.mesh_mut().tag_curve(&pts, true);
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: vp.scene.active_mesh_id(),
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
    (vp, f)
}

#[test]
fn test_passing_over_the_circle_face_leaves_the_centre_as_a_reference_and_snaps() {
    let (mut vp, _) = circle_view();
    vp.update_center_ref(30.0, 20.0); // the cursor over the face
    let r = vp.center_ref().cloned().expect("a centre");
    assert!((r.center - v(3., 2.)).length() < 1e-6);
    // the centre reaches the snap engine as a «center» pseudo-edge
    let ss = vp.snap_scene(Some((30.0, 20.0)));
    assert_eq!(ss.iter().filter(|e| e.center).count(), 1);
    let w2p = |p: DVec3| Some((p.x * 10.0, p.y * 10.0));
    let occluded = |_: DVec3| false;
    let mut q = SnapQuery::new(v(3.05, 2.02), (30.5, 20.2), &w2p, 9.0);
    q.is_occluded = Some(&occluded);
    let snap = compute_snap(&q, &ss);
    assert!(snap.kind == SnapKind::Center && (snap.point - v(3., 2.)).length() < 1e-6);
    // with nothing under the cursor the reference stays (it goes when the tool changes)
    FakeProbe::of(&mut vp).pick(None);
    vp.update_center_ref(500.0, 500.0);
    assert!(vp.center_ref().is_some());
}

#[test]
fn test_the_curve_edge_gives_the_centre_too() {
    let mut vp = plan_view();
    let pts = circle_pts(3.0, 2.0, 1.5, 24);
    vp.scene.mesh_mut().add_face(&pts, &[]);
    vp.scene.mesh_mut().tag_curve(&pts, true);
    let m = vp.scene.mesh();
    let e = m
        .edge_ids()
        .into_iter()
        .find(|&e| m.edge(e).curve.is_some())
        .expect("a curve edge");
    common::set_hover_edge(&mut vp, e);
    vp.update_center_ref(0.0, 0.0);
    let r = vp.center_ref().cloned().expect("a centre");
    assert!((r.center - v(3., 2.)).length() < 1e-6);
    assert!(matches!(r.key, CenterKey::Edge(_)));
}

#[test]
fn test_erasing_the_circle_takes_the_green_dot_and_moving_it_drags_it() {
    // The centre reference is revalidated on every scene change: an erased
    // circle's green point does not linger.
    let (mut vp, f) = circle_view();
    vp.update_center_ref(30.0, 20.0);
    assert!(vp.center_ref().is_some());
    // move the whole circle: the centre follows it
    for vid in vp.scene.mesh().vertex_ids() {
        vp.scene.mesh_mut().move_vertex(vid, v(10., 0.));
    }
    vp.scene.version += 1;
    let r = vp.valid_center_ref().expect("still there");
    assert!((r.center - v(13., 2.)).length() < 1e-6);
    // erase it: the point goes
    let cmd = EraseSelectionCommand::from_entities(vp.scene.mesh(), &[], &[f]);
    vp.run(cmd);
    assert!(vp.valid_center_ref().is_none());
    assert!(!vp.snap_scene(Some((30.0, 20.0))).iter().any(|e| e.center));
}

#[test]
fn test_a_circle_tiny_on_screen_offers_no_centre() {
    // Seen from far away, a small circle must not offer its centre: the hint
    // is a share of the circle's size on screen, capped; a circle under
    // 12 px in radius offers no centre.
    let (mut vp, _) = circle_view();
    vp.update_center_ref(30.0, 20.0);
    let r = vp.center_ref().cloned().expect("a centre");
    let pc = vp.world_to_pixel(r.center).unwrap();
    // at 10 px/m: radius 15 px → hint at half, 7.5 px
    assert!((vp.center_hint_px(&r, pc.tuple()) - 7.5).abs() < 1e-6);
    assert!(vp.snap_scene(Some((30.0, 20.0))).iter().any(|e| e.center));
    // from far (1 px/m): radius 1.5 px → nothing
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x, p.y)));
    let pc = vp.world_to_pixel(r.center).unwrap();
    assert_eq!(vp.center_hint_px(&r, pc.tuple()), 0.0);
    assert!(!vp.snap_scene(Some((3.0, 2.0))).iter().any(|e| e.center));
    // from close (100 px/m): radius 150 px → capped at 40
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x * 100.0, p.y * 100.0)));
    let pc = vp.world_to_pixel(r.center).unwrap();
    assert_eq!(vp.center_hint_px(&r, pc.tuple()), 40.0);
}
