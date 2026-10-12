// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Loops of a few mm² (screw holes, small squares) make faces like any
//! other: drawn on the ground, or on a face they split and can be pushed
//! through.

use crate::common;

use common::{add_box, push, stub, v3};
use glam::DVec3;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;

/// An M3 clearance hole's radius, in metres.
const M3: f64 = 0.0016;
/// The box: 60 × 40 × 8 mm.
const W: f64 = 0.060;
const D: f64 = 0.040;
const T: f64 = 0.008;

fn click(t: &mut dyn Tool, vp: &mut Viewport, p: DVec3) {
    t.on_click(vp, &ToolContext::at(p));
}

fn circle(vp: &mut Viewport, c: DVec3, r: f64) {
    let mut t = RadialTool::circle();
    let rim = c + DVec3::X * r;
    click(&mut t, vp, c);
    t.on_hover(vp, &ToolContext::at(rim));
    click(&mut t, vp, rim);
}

/// The faces whose outer loop has `n` corners, all at height `z`.
fn faces_at(vp: &Viewport, n: usize, z: f64) -> Vec<xtrude_core::FaceId> {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| {
            let pts = m.face_positions(f);
            pts.len() == n && pts.iter().all(|p| (p.z - z).abs() < 1e-6)
        })
        .collect()
}

#[test]
fn test_an_m3_circle_on_the_ground_makes_a_24_sided_face() {
    let mut vp = stub();
    circle(&mut vp, DVec3::ZERO, M3);
    let m = vp.scene.mesh();
    let faces = m.face_ids();
    assert_eq!(faces.len(), 1);
    let pts = m.face_positions(faces[0]);
    assert_eq!(pts.len(), 24);
    assert!(pts.iter().all(|p| (p.length() - M3).abs() < 1e-6));
}

#[test]
fn test_an_m3_circle_on_a_box_splits_its_top_and_pushes_through() {
    let mut vp = stub();
    add_box(&mut vp, DVec3::ZERO, v3(W, D, T));
    let centre = v3(W / 2.0, D / 2.0, T);
    circle(&mut vp, centre, M3);
    assert!(
        vp.history.last_error.is_none(),
        "{:?}",
        vp.history.last_error
    );

    let disc = faces_at(&vp, 24, T);
    assert_eq!(disc.len(), 1, "the circle makes its own face");
    let top = faces_at(&vp, 4, T);
    assert_eq!(top.len(), 1);
    assert_eq!(vp.scene.mesh().face(top[0]).holes.len(), 1);

    let n = vp.scene.mesh().face_normal(disc[0]);
    let dist = if n.z > 0.0 { -T } else { T };
    assert!(push(&mut vp, disc[0], dist, false));

    let m = vp.scene.mesh();
    assert!(faces_at(&vp, 24, T).is_empty());
    assert!(faces_at(&vp, 24, 0.0).is_empty());
    for z in [0.0, T] {
        let caps = faces_at(&vp, 4, z);
        assert_eq!(caps.len(), 1);
        assert_eq!(m.face(caps[0]).holes.len(), 1, "a hole at z = {z}");
    }
    // The bore: 24 walls from the bottom to the top.
    let walls = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let pts = m.face_positions(f);
            pts.len() == 4
                && pts.iter().all(|p| {
                    (DVec3::new(p.x, p.y, 0.0) - v3(W / 2.0, D / 2.0, 0.0)).length() < M3 + 1e-6
                })
        })
        .count();
    assert_eq!(walls, 24);
}

#[test]
fn test_a_9_mm_square_on_the_ground_makes_a_face() {
    let mut vp = stub();
    let mut t = RectangleTool::new();
    for p in [v3(0.0, 0.0, 0.0), v3(0.009, 0.009, 0.0)] {
        click(&mut t, &mut vp, p);
    }
    let m = vp.scene.mesh();
    let faces = m.face_ids();
    assert_eq!(faces.len(), 1);
    assert!((m.face_area(faces[0]) - 0.009 * 0.009).abs() < 1e-10);
}
