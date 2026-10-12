// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotate + Ctrl, then "3x" / "/3" — a polar array.

use crate::common;
use xtrude_app::value_box::ArrayKind;

use common::stub;
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn ctx(x: f64, y: f64) -> ToolContext {
    ToolContext::at(DVec3::new(x, y, 0.0))
}

/// A little square centred on the +X arm at (2, 0).
fn marker(vp: &mut Viewport) {
    let h = 0.1;
    let (x, y) = (2.0, 0.0);
    let f = vp.scene.mesh_mut().add_face(
        &[
            DVec3::new(x - h, y - h, 0.),
            DVec3::new(x + h, y - h, 0.),
            DVec3::new(x + h, y + h, 0.),
            DVec3::new(x - h, y + h, 0.),
        ],
        &[],
    );
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
}

/// Angle (degrees, 0–360) of each face's centroid about the origin.
fn angles(vp: &Viewport) -> Vec<f64> {
    let m = vp.scene.mesh();
    let mut out: Vec<f64> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let c = m.face_centroid(f);
            (c.y.atan2(c.x).to_degrees().rem_euclid(360.0) * 1000.0).round() / 1000.0
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

fn rotate_copy(vp: &mut Viewport, t: &mut RotateTool, deg: f64) {
    t.on_click(vp, &ctx(0., 0.));
    t.on_click(vp, &ctx(2., 0.));
    t.on_key(vp, Key::Control, Modifiers::NONE);
    let r = 2.0;
    t.on_click(
        vp,
        &ctx(r * deg.to_radians().cos(), r * deg.to_radians().sin()),
    );
}

#[test]
fn test_the_tool_declares_arrays_so_the_value_box_keeps_the_x() {
    let t = RotateTool::new();
    // One flag now says both "the box keeps the x" and "the tool lays it".
    assert!(t.value_spec().array.is_some());
}

#[test]
fn test_external_array_lays_copies_at_multiples_of_the_angle() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    rotate_copy(&mut vp, &mut t, 30.0);
    assert_eq!(angles(&vp), vec![0.0, 30.0]);
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::External, None));
    assert_eq!(angles(&vp), vec![0.0, 30.0, 60.0, 90.0]);
}

#[test]
fn test_internal_array_divides_the_angle() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    rotate_copy(&mut vp, &mut t, 90.0);
    assert_eq!(angles(&vp), vec![0.0, 90.0]);
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::Internal, None));
    assert_eq!(angles(&vp), vec![0.0, 30.0, 60.0, 90.0]);
}

#[test]
fn test_retyping_re_lays_the_array_and_it_is_one_undo_step() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    rotate_copy(&mut vp, &mut t, 30.0);
    t.on_array_value(&mut vp, 3, ArrayKind::External, None);
    assert_eq!(angles(&vp).len(), 4);
    t.on_array_value(&mut vp, 5, ArrayKind::External, None);
    assert_eq!(angles(&vp), vec![0.0, 30.0, 60.0, 90.0, 120.0, 150.0]);
    vp.undo();
    assert_eq!(angles(&vp), vec![0.0]);
}

#[test]
fn test_the_angle_keeps_its_side() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    rotate_copy(&mut vp, &mut t, -30.0);
    assert!(t.on_array_value(&mut vp, 2, ArrayKind::External, None));
    assert_eq!(angles(&vp), vec![0.0, 300.0, 330.0]);
}

#[test]
fn test_a_plain_rotation_offers_no_array() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_click(&mut vp, &ctx(2., 0.));
    t.on_click(&mut vp, &ctx(0., 2.));
    assert_eq!(angles(&vp), vec![90.0]);
    assert!(!t.on_array_value(&mut vp, 3, ArrayKind::External, None));
    assert_eq!(angles(&vp), vec![90.0]);
}

#[test]
fn test_the_array_window_closes_when_something_else_is_undone() {
    let mut vp = stub();
    marker(&mut vp);
    let mut t = RotateTool::new();
    rotate_copy(&mut vp, &mut t, 30.0);
    vp.undo();
    assert!(!t.on_array_value(&mut vp, 3, ArrayKind::External, None));
}
