// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Pushing a rim piece right through a solid trims the far face to what
//! remains — a notch, not a hole — with no cap left behind; short of the far
//! face the push is a blind recess.

use crate::common;

use common::{draw_rect, sized_viewport, v3};
use glam::DVec3;
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;
use xtrude_app::tools::arc::commit_arc;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::viewport::Viewport;
use xtrude_core::orient::is_closed;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::ids::MeshId;

fn tool_on(vp: &Viewport, face: FaceId) -> PushPullTool {
    let mut t = PushPullTool::new();
    t.pp.setup_plain(vp.scene.mesh(), MeshId::LOOSE, face);
    t
}

fn push(vp: &mut Viewport, face: FaceId, dist: f64) {
    let mut t = tool_on(vp, face);
    t.pp.limit_in = None;
    t.pp.extrusion = dist;
    t.commit(vp);
}

fn area(m: &Mesh, f: FaceId) -> f64 {
    m.loop_area(&m.face(f).outer)
}

fn faces_where(m: &Mesh, pred: impl Fn(DVec3) -> bool) -> Vec<FaceId> {
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|&p| pred(p)))
        .collect()
}

/// A 2 m cube whose front face (y = 0) has its top-right corner rounded by
/// an arc of radius 0.5; the viewport and the corner sliver.
fn rounded_box() -> (Viewport, FaceId) {
    let mut vp = sized_viewport(1000., 700.);
    draw_rect(
        &mut vp,
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
    );
    let f = vp.scene.mesh().face_ids()[0];
    push(&mut vp, f, 2.0);
    let (c, r) = (v3(1.5, 0., 1.5), 0.5);
    let pts: Vec<DVec3> = (0..9)
        .map(|k| {
            let a = FRAC_PI_2 * k as f64 / 8.0;
            c + v3(r * a.cos(), 0., r * a.sin())
        })
        .collect();
    commit_arc(&mut vp, &pts, None, None);
    let m = vp.scene.mesh();
    let front = faces_where(m, |p| p.y.abs() < 1e-9);
    assert_eq!(front.len(), 2);
    let sliver = *front
        .iter()
        .min_by(|&&a, &&b| area(m, a).total_cmp(&area(m, b)))
        .unwrap();
    (vp, sliver)
}

fn back(m: &Mesh) -> Vec<FaceId> {
    faces_where(m, |p| (p.y - 2.0).abs() < 1e-6)
}

#[test]
fn test_pushing_the_corner_piece_through_trims_the_far_face() {
    let (mut vp, sliver) = rounded_box();
    push(&mut vp, sliver, -2.0);
    let m = vp.scene.mesh();
    assert!(is_closed(m));
    let bk = back(m);
    assert!(bk.len() == 1 && m.face(bk[0]).outer.len() == 12); // the arc outline
                                                               // No coplanar leftover at the back, no cap: 6 box faces + 8 strip quads.
    assert_eq!(m.face_ids().len(), 6 + 8);
    let top = faces_where(m, |p| (p.z - 2.0).abs() < 1e-6);
    assert_eq!(top.len(), 1);
    let max_x = m
        .face_positions(top[0])
        .iter()
        .map(|p| p.x)
        .fold(f64::MIN, f64::max);
    assert!((max_x - 1.5).abs() < 1e-6);
    let right = faces_where(m, |p| (p.x - 2.0).abs() < 1e-6);
    assert_eq!(right.len(), 1);
    let max_z = m
        .face_positions(right[0])
        .iter()
        .map(|p| p.z)
        .fold(f64::MIN, f64::max);
    assert!((max_z - 1.5).abs() < 1e-6);
    // The strip runs the whole depth.
    let strip = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let ps = m.face_positions(f);
            let xs: BTreeSet<i64> = ps.iter().map(|p| (p.x * 1e6).round() as i64).collect();
            let zs: BTreeSet<i64> = ps.iter().map(|p| (p.z * 1e6).round() as i64).collect();
            ps.len() == 4
                && ps.iter().any(|p| p.y.abs() < 1e-9)
                && ps.iter().any(|p| (p.y - 2.0).abs() < 1e-9)
                && xs.len() > 1
                && zs.len() > 1
        })
        .count();
    assert_eq!(strip, 8);
    // One undo brings the sliver back.
    vp.undo();
    let m = vp.scene.mesh();
    assert_eq!(m.face(back(m)[0]).outer.len(), 4);
}

#[test]
fn test_pushing_past_the_far_face_stops_flush_and_trims_too() {
    let (mut vp, sliver) = rounded_box();
    let mut t = tool_on(&vp, sliver);
    t.pp.compute_inward_limit(vp.scene.mesh());
    assert!((t.pp.limit_in.unwrap() - 2.0).abs() < 1e-6);
    t.pp.extrusion = -2.6; // overshoot: clamps to the back
    t.pp.clamp_extrusion();
    assert!((t.pp.extrusion + 2.0).abs() < 1e-6);
    t.commit(&mut vp);
    let m = vp.scene.mesh();
    assert!(is_closed(m) && m.face_ids().len() == 14);
    assert_eq!(m.face(back(m)[0]).outer.len(), 12);
}

#[test]
fn test_short_of_the_far_face_is_still_a_recess() {
    let (mut vp, sliver) = rounded_box();
    push(&mut vp, sliver, -1.94);
    let m = vp.scene.mesh();
    assert!(is_closed(m));
    assert_eq!(m.face(back(m)[0]).outer.len(), 4); // untouched
    assert_eq!(faces_where(m, |p| (p.y - 1.94).abs() < 1e-6).len(), 1);
}
