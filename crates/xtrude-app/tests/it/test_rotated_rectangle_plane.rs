// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Rotated Rectangle draws in a PLANE; its third step
//! is width AND angle; an axis lock beats the snap.

use crate::common;
use crate::common::FakeProbe;
use xtrude_core::Axis;

use common::{stub, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::rotated_rectangle::RotatedRectangleTool;
use xtrude_app::tools::{plane_lock_axis, Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{FaceHit, Viewport, ViewportEvent};
use xtrude_core::Mesh;
use xtrude_model::group::add_mesh;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

fn said(vp: &mut Viewport) -> Vec<String> {
    vp.outbox
        .events
        .drain(..)
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

/// A wall in the XZ plane under the cursor (a face of a mesh of its own).
fn on_wall(vp: &mut Viewport) {
    let mut m = Mesh::new();
    let f = m.add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 0., 3.),
            v3(0., 0., 3.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: mid,
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
}

fn off_plane(corners: &[DVec3], origin: DVec3, n: DVec3) -> Vec<f64> {
    corners
        .iter()
        .map(|p| ((*p - origin).dot(n.normalize()) * 1e6).round() / 1e6)
        .collect()
}

fn key_for(axis: Axis) -> Key {
    [Key::Right, Key::Left, Key::Up]
        .into_iter()
        .find(|k| plane_lock_axis(*k) == Some(axis))
        .unwrap()
}

#[test]
fn test_it_is_a_plane_locking_tool_like_every_other_planar_one() {
    let t = RotatedRectangleTool::new();
    assert!(t.work_plane().is_some_and(|w| w.lock.is_some()));
}

#[test]
fn test_the_arrow_keys_reach_it() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    assert!(tool.on_key(&mut vp, key_for(Axis::Y), Modifiers::NONE));
    assert_eq!(tool.lock.plane_lock, Some(Axis::Y));
    tool.reset();
    assert!(
        tool.lock.plane_lock.is_none(),
        "the lock must not outlive the shape"
    );
}

#[test]
fn test_the_first_click_captures_the_face_it_landed_on() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    on_wall(&mut vp);
    tool.on_click(&mut vp, &ctx(v3(1., 0., 1.)));
    let (_p, n) = tool
        .work_plane
        .expect("the face under the click was ignored");
    assert!((n.normalize().y.abs() - 1.0).abs() < 1e-6);
}

#[test]
fn test_a_rectangle_on_a_wall_stays_on_the_wall() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    on_wall(&mut vp);
    tool.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    tool.on_click(&mut vp, &ctx(v3(3., 0., 0.)));
    assert!(tool.base_point.is_some());
    let corners = tool.corners(1.0, None);
    assert!(!corners.is_empty());
    assert_eq!(
        off_plane(&corners, DVec3::ZERO, v3(0., 1., 0.)),
        vec![0.0; 4]
    );
}

#[test]
fn test_a_vertical_base_edge_works_on_a_wall() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    on_wall(&mut vp);
    tool.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    tool.on_click(&mut vp, &ctx(v3(0., 0., 3.)));
    assert!(tool.base_point.is_some());
    assert!(!tool.corners(1.0, None).is_empty());
    assert!(said(&mut vp).is_empty());
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_an_edge_perpendicular_to_the_plane_is_refused_OUT_LOUD() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub(); // no face: the ground plane
    tool.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    assert!(tool.work_plane.is_none());
    tool.on_click(&mut vp, &ctx(v3(0., 0., 3.))); // straight up, off the ground
    assert!(
        tool.base_point.is_none(),
        "it accepted an impossible base edge"
    );
    assert!(!said(&mut vp).is_empty(), "it refused without a word");
}

#[test]
fn test_the_ground_case_is_unchanged() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    tool.on_click(&mut vp, &ctx(v(0., 0.)));
    tool.on_click(&mut vp, &ctx(v(3., 0.)));
    assert_eq!(
        tool.corners(2.0, None),
        vec![v(0., 0.), v(3., 0.), v(3., 2.), v(0., 2.)]
    );
}

#[test]
fn test_an_arrow_lock_beats_the_face_under_the_cursor() {
    let mut tool = RotatedRectangleTool::new();
    let mut vp = stub();
    on_wall(&mut vp); // a wall says XZ...
    tool.on_key(&mut vp, key_for(Axis::Z), Modifiers::NONE); // ...the user asked XY
    tool.on_click(&mut vp, &ctx(v(0., 0.)));
    let (_p, n) = tool.work_plane.unwrap();
    assert!((n.normalize().z.abs() - 1.0).abs() < 1e-6);
}

// ---- the third step is width AND angle (the classic protractor) ----------

fn with_base(a: DVec3, b: DVec3) -> (RotatedRectangleTool, Viewport) {
    let mut vp = stub();
    let mut tool = RotatedRectangleTool::new();
    tool.on_click(&mut vp, &ctx(a));
    tool.on_click(&mut vp, &ctx(b));
    (tool, vp)
}

fn zs(vp: &Viewport) -> Vec<f64> {
    let m = vp.scene.mesh();
    let mut z: Vec<f64> = m
        .face_positions(m.face_ids()[0])
        .iter()
        .map(|p| (p.z * 1e3).round() / 1e3)
        .collect();
    z.sort_by(f64::total_cmp);
    z.dedup();
    z
}

#[test]
fn test_typing_width_and_angle_raises_the_rectangle() {
    let _m = MetreDocument::new();
    let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
    tool.hover_point = Some(v(9., 1.));
    assert!(tool.on_value(&mut vp, &ToolValue::Pair(3.0, 90.0)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert_eq!(zs(&vp), vec![0.0, 3.0]);
    let m = vp.scene.mesh();
    assert!(m.face_normal(m.face_ids()[0]).z.abs() < 1e-6); // standing up
}

#[test]
fn test_a_single_number_keeps_the_current_angle() {
    let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
    tool.hover_point = Some(v(9., 1.));
    tool.angle = 90.0;
    assert!(tool.on_value(&mut vp, &ToolValue::Num(2.0)));
    assert_eq!(zs(&vp), vec![0.0, 2.0]);
}

#[test]
fn test_the_cursor_off_the_plane_gives_the_angle_alone() {
    let (tool, _vp) = with_base(v(0., 0.), v(9., 0.));
    let (w, a) = tool.width_and_angle(v3(9., 0., 2.5), None);
    assert!((w - 2.5).abs() < 1e-6 && (a - 90.0).abs() < 1e-6);
    let (w, a) = tool.width_and_angle(v(9., 1.5), None);
    assert!((w - 1.5).abs() < 1e-6);
    assert!(a.abs() < 1e-6, "in the plane the angle is 0");
}

#[test]
fn test_the_angle_sticks_to_flat_and_perpendicular() {
    let (tool, _vp) = with_base(v(0., 0.), v(9., 0.));
    let r = |deg: f64| deg.to_radians();
    // 88.5° is inside the 3° magnet: it lands on 90 exactly.
    let nearly = v3(9., 1.5 * r(88.5).cos(), 1.5 * r(88.5).sin());
    assert!((tool.width_and_angle(nearly, None).1 - 90.0).abs() < 1e-9);
    // 60° is left alone.
    let far = v3(9., 1.5 * r(60.).cos(), 1.5 * r(60.).sin());
    assert!((tool.width_and_angle(far, None).1 - 60.0).abs() < 0.01);
}

#[test]
fn test_zero_width_warns_instead_of_breaking() {
    let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
    tool.on_click(&mut vp, &ctx(v(4., 0.))); // on the base itself
    assert!(vp.scene.mesh().faces.is_empty());
    assert!(vp.history.last_error.is_none(), "it must not even throw");
    assert!(!said(&mut vp).is_empty(), "and it must say why");
}

#[test]
fn test_the_flat_rectangle_does_not_change() {
    let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
    tool.on_click(&mut vp, &ctx(v(9., 1.13)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(vp.history.last_error.is_none());
    assert_eq!(zs(&vp), vec![0.0]);
}

// ---- the axis lock beats the snap -----------------------------------------

fn locked(lock: Option<Axis>, a: DVec3, b: DVec3) -> (RotatedRectangleTool, Viewport) {
    let mut vp = stub();
    vp.inference.axis_lock = lock;
    let mut tool = RotatedRectangleTool::new();
    tool.on_click(&mut vp, &ctx(a));
    tool.on_click(&mut vp, &ctx(b));
    (tool, vp)
}

fn default_base(lock: Option<Axis>) -> (RotatedRectangleTool, Viewport) {
    locked(lock, v(7.2, 21.4), v(16.2, 21.4))
}

#[test]
fn test_the_snap_cannot_lay_down_a_rectangle_with_the_axis_locked() {
    let (mut tool, mut vp) = default_base(Some(Axis::Z));
    tool.on_hover(&mut vp, &ctx(v3(16.2, 21.4, 1.05))); // the mouse, up
    tool.on_click(&mut vp, &ctx(v3(16.2, 22.0, 0.0))); // the snap, on the floor
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert_eq!(zs(&vp), vec![0.0, 1.05]);
    let m = vp.scene.mesh();
    let ys: std::collections::BTreeSet<i64> = m
        .face_positions(m.face_ids()[0])
        .iter()
        .map(|p| (p.y * 1e3).round() as i64)
        .collect();
    assert_eq!(ys, [21400].into_iter().collect());
}

#[test]
fn test_without_a_lock_the_click_still_rules() {
    let (mut tool, mut vp) = default_base(None);
    tool.on_hover(&mut vp, &ctx(v3(16.2, 21.4, 1.05)));
    tool.on_click(&mut vp, &ctx(v3(16.2, 22.0, 0.0)));
    assert_eq!(zs(&vp), vec![0.0]);
}

#[test]
fn test_the_locked_axis_fixes_the_width_direction() {
    let (tool, vp) = default_base(Some(Axis::Z));
    let d = tool.locked_dir(&vp).expect("a direction");
    assert!((d.z - 1.0).abs() < 1e-9, "with Z locked the width goes up");
    let (tool2, vp2) = default_base(Some(Axis::X));
    // The base runs along X, so an X lock leaves no direction: ignored.
    assert!(tool2.locked_dir(&vp2).is_none());
}

#[test]
fn test_typing_only_the_width_keeps_the_angle_on_screen() {
    let (mut tool, mut vp) = locked(Some(Axis::Z), v(7.2, 21.4), v(16.2, 21.404));
    tool.on_hover(&mut vp, &ctx(v3(7.2, 21.4, 2.017)));
    assert!(
        (tool.angle - 90.0).abs() < 1e-6,
        "the hover must remember the angle"
    );
    assert!(tool.on_value(&mut vp, &ToolValue::Num(0.6)));
    assert_eq!(zs(&vp), vec![0.0, 0.6]);
}

#[test]
fn test_the_hover_does_not_overwrite_the_angle_with_zero_width() {
    let (mut tool, mut vp) = default_base(Some(Axis::Z));
    tool.on_hover(&mut vp, &ctx(v3(16.2, 21.4, 1.5)));
    assert!((tool.angle - 90.0).abs() < 1e-6);
    tool.on_hover(&mut vp, &ctx(v3(12.0, 21.4, 0.0))); // over the base
    assert!((tool.angle - 90.0).abs() < 1e-6, "the angle was lost");
}

#[test]
fn test_typing_the_width_puts_it_on_the_cursor_side() {
    for (side, expected) in [(-3.0, vec![-2.0, 0.0]), (3.0, vec![0.0, 2.0])] {
        let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
        tool.on_hover(&mut vp, &ctx(v(4.5, side)));
        assert!(tool.on_value(&mut vp, &ToolValue::Num(2.0)));
        let m = vp.scene.mesh();
        let mut ys: Vec<f64> = m
            .face_positions(m.face_ids()[0])
            .iter()
            .map(|p| (p.y * 1e3).round() / 1e3)
            .collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup();
        assert_eq!(ys, expected, "cursor at y={side}");
    }
}

#[test]
fn test_typing_the_width_downward_does_not_raise_the_rectangle() {
    let (mut tool, mut vp) = with_base(v(0., 0.), v(9., 0.));
    tool.on_hover(&mut vp, &ctx(v3(4.5, 0.0, -2.5)));
    assert!((tool.angle + 90.0).abs() < 1e-6);
    assert!(tool.on_value(&mut vp, &ToolValue::Num(1.0)));
    assert_eq!(zs(&vp), vec![-1.0, 0.0]);
}
