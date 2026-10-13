// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Centre Rectangle: the centre, then a corner; a typed `w;h` centred on
//! the first click. One undo step; Ctrl switches to corner-wise.

use crate::common;

use std::collections::BTreeSet;

use common::stub as viewport;
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_app::window::make_tool;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// The corners of the one face, rounded to 1e-6.
fn corners(vp: &Viewport) -> BTreeSet<(i64, i64, i64)> {
    let m = vp.scene.mesh();
    assert_eq!(m.face_ids().len(), 1);
    m.face_positions(m.face_ids()[0])
        .into_iter()
        .map(|p| {
            let r = |x: f64| (x * 1e6).round() as i64;
            (r(p.x), r(p.y), r(p.z))
        })
        .collect()
}

fn keys(pts: &[(f64, f64)]) -> BTreeSet<(i64, i64, i64)> {
    pts.iter()
        .map(|p| ((p.0 * 1e6).round() as i64, (p.1 * 1e6).round() as i64, 0))
        .collect()
}

fn click(t: &mut RectangleTool, vp: &mut Viewport, p: DVec3) {
    let c = ToolContext::at(p);
    t.on_hover(vp, &c);
    t.on_click(vp, &c);
}

#[test]
fn test_the_centre_then_a_corner() {
    let mut vp = viewport();
    let mut t = RectangleTool::centre();
    t.on_activate(&mut vp);
    let depth = vp.history.undo_len();
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(10.0, 5.0));
    assert_eq!(
        corners(&vp),
        keys(&[(-10., -5.), (10., -5.), (10., 5.), (-10., 5.)])
    );
    assert_eq!(vp.history.undo_len(), depth + 1);
    // it faces up, as the plane does
    let m = vp.scene.mesh();
    assert!(m.face_normal(m.face_ids()[0]).z > 0.99);
}

#[test]
fn test_a_typed_size_is_centred_on_the_first_click() {
    let mut vp = viewport();
    let mut t = RectangleTool::centre();
    t.on_activate(&mut vp);
    click(&mut t, &mut vp, v(1.0, 1.0));
    t.on_hover(&mut vp, &ToolContext::at(v(3.0, 2.0)));
    assert!(t.on_value(&mut vp, &ToolValue::Pair(4.0, 2.0)));
    assert_eq!(
        corners(&vp),
        keys(&[(-1., 0.), (3., 0.), (3., 2.), (-1., 2.)])
    );
}

#[test]
fn test_ctrl_draws_from_a_corner_and_the_tool_keeps_its_key() {
    let mut vp = viewport();
    let mut t = RectangleTool::centre();
    t.on_activate(&mut vp);
    assert!(t.from_center);
    assert_eq!(t.info().icon, Some("rectangle_center"));
    assert!(t.on_key(&mut vp, Key::Control, Modifiers::NONE));
    assert!(!t.from_center);
    assert_eq!(t.info().key, "rectangle_center");
    assert_eq!(t.info().icon, Some("rectangle"));
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(2.0, 1.0));
    assert_eq!(
        corners(&vp),
        keys(&[(0., 0.), (2., 0.), (2., 1.), (0., 1.)])
    );
    // picked up again, it starts from the centre
    t.on_activate(&mut vp);
    assert!(t.from_center);
}

#[test]
fn test_the_rectangle_tool_still_starts_from_a_corner() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    assert!(!t.from_center);
    assert_eq!(t.info().key, "rectangle");
}

#[test]
fn test_the_window_makes_it_under_its_own_key() {
    let t = make_tool("rectangle_center").expect("a Centre Rectangle");
    assert_eq!(t.info().key, "rectangle_center");
    assert_eq!(t.info().name, "Centre Rectangle");
    assert_eq!(t.info().icon, Some("rectangle_center"));
    let mut w = common::window();
    w.trigger("rectangle_center");
    assert_eq!(w.active_tool_key(), Some("rectangle_center"));
    assert_eq!(w.stack_current("rectangles"), Some("rectangle_center"));
}
