// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Slot: the first centre, the second, then the width (or typed
//! `length;width`): one obround face, its ends tagged as two arcs, one
//! undo step.

use crate::common;

use std::f64::consts::PI;

use common::stub as viewport;
use glam::DVec3;
use xtrude_app::tools::slot::{slot_outline, SlotTool, SLOT_SIDES};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_app::window::make_tool;

const MM: f64 = 0.001;

fn mm(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0) * MM
}

fn click(t: &mut SlotTool, vp: &mut Viewport, p: DVec3) {
    let c = ToolContext::at(p);
    t.on_hover(vp, &c);
    t.on_click(vp, &c);
}

/// The one face's size along X and Y, and its area, in mm.
fn face_size(vp: &Viewport) -> (f64, f64, f64) {
    let m = vp.scene.mesh();
    assert_eq!(m.face_ids().len(), 1);
    let f = m.face_ids()[0];
    let pts = m.face_positions(f);
    let span = |k: fn(&DVec3) -> f64| {
        let hi = pts.iter().map(k).fold(f64::MIN, f64::max);
        let lo = pts.iter().map(k).fold(f64::MAX, f64::min);
        (hi - lo) / MM
    };
    (span(|p| p.x), span(|p| p.y), m.face_area(f) / (MM * MM))
}

/// 20 × 6 mm plus the two half circles of radius 3.
fn check_20_by_6(vp: &Viewport) {
    let (w, h, area) = face_size(vp);
    assert!((w - 26.0).abs() < 1e-6, "{w}");
    assert!((h - 6.0).abs() < 1e-6, "{h}");
    let round = 20.0 * 6.0 + PI * 3.0 * 3.0;
    // a 24-gon of radius 3 falls short of the circle by 0.32 mm²
    assert!(area < round && round - area < 0.33, "{area} vs {round}");
}

#[test]
fn test_two_centres_and_the_width() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    let depth = vp.history.undo_len();
    click(&mut t, &mut vp, mm(0.0, 0.0));
    assert_eq!(t.phase(), "started");
    click(&mut t, &mut vp, mm(20.0, 0.0));
    assert_eq!(t.phase(), "width");
    // the cursor sets half the width, measured from the segment
    t.on_hover(&mut vp, &ToolContext::at(mm(8.0, 3.0)));
    let (label, _) = t.value_label(&vp).unwrap();
    assert!(label.contains('×'), "{label}");
    click(&mut t, &mut vp, mm(8.0, 3.0));
    check_20_by_6(&vp);
    assert_eq!(vp.history.undo_len(), depth + 1);
    assert_eq!(t.phase(), "idle");
    let m = vp.scene.mesh();
    assert!(m.face_normal(m.face_ids()[0]).z > 0.99);
}

#[test]
fn test_typed_length_and_width_toward_the_cursor() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    click(&mut t, &mut vp, mm(0.0, 0.0));
    t.on_hover(&mut vp, &ToolContext::at(mm(5.0, 0.0)));
    assert!(t.on_value(&mut vp, &ToolValue::Pair(20.0 * MM, 6.0 * MM)));
    check_20_by_6(&vp);
    let m = vp.scene.mesh();
    let xs: Vec<f64> = m.verts.iter().map(|(_, v)| v.position.x / MM).collect();
    let lo = xs.iter().copied().fold(f64::MAX, f64::min);
    assert!((lo + 3.0).abs() < 1e-6, "starts at the first centre: {lo}");
}

#[test]
fn test_typed_length_then_typed_width() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    click(&mut t, &mut vp, mm(0.0, 0.0));
    t.on_hover(&mut vp, &ToolContext::at(mm(5.0, 0.0)));
    assert!(t.on_value(&mut vp, &ToolValue::Num(20.0 * MM)));
    assert_eq!(t.phase(), "width");
    assert!(t.on_value(&mut vp, &ToolValue::Num(6.0 * MM)));
    check_20_by_6(&vp);
}

#[test]
fn test_the_ends_are_two_arcs_of_the_circle_side_rule() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    click(&mut t, &mut vp, mm(0.0, 0.0));
    click(&mut t, &mut vp, mm(20.0, 0.0));
    click(&mut t, &mut vp, mm(10.0, 3.0));
    let m = vp.scene.mesh();
    assert_eq!(m.edge_ids().len(), SLOT_SIDES + 2);
    let curves: Vec<i64> = m.edges.iter().filter_map(|(_, e)| e.curve).collect();
    assert_eq!(curves.len(), SLOT_SIDES, "the straight sides are no curve");
    let mut ids = curves.clone();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2, "one curve per end");
}

#[test]
fn test_typed_sides_before_the_first_click() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    assert!(t.on_value(&mut vp, &ToolValue::Segments(32)));
    click(&mut t, &mut vp, mm(0.0, 0.0));
    click(&mut t, &mut vp, mm(20.0, 0.0));
    click(&mut t, &mut vp, mm(10.0, 3.0));
    assert_eq!(vp.scene.mesh().edge_ids().len(), 34);
}

#[test]
fn test_a_second_centre_on_the_first_is_refused() {
    let mut vp = viewport();
    let mut t = SlotTool::new();
    t.on_activate(&mut vp);
    click(&mut t, &mut vp, mm(1.0, 1.0));
    click(&mut t, &mut vp, mm(1.0, 1.0));
    assert_eq!(t.phase(), "started");
    assert!(vp.scene.mesh().face_ids().is_empty());
}

#[test]
fn test_the_outline_is_counter_clockwise_and_closed_by_its_sides() {
    let o = slot_outline(mm(0.0, 0.0), mm(20.0, 0.0), 3.0 * MM, DVec3::Z, 24).unwrap();
    assert_eq!(o.points.len(), 26);
    assert_eq!(o.arcs[0].len(), 13);
    // the sides join the arcs' ends
    assert!((o.arcs[0][12] - mm(20.0, 3.0)).length() < 1e-12);
    assert!((o.arcs[1][0] - mm(0.0, 3.0)).length() < 1e-12);
    let n = xtrude_app::tools::loop_normal(&o.points);
    assert!(n.z > 0.0);
    assert!(slot_outline(mm(0.0, 0.0), mm(0.0, 0.0), 3.0 * MM, DVec3::Z, 24).is_none());
}

#[test]
fn test_the_window_makes_it() {
    let t = make_tool("slot").expect("a Slot tool");
    assert_eq!(t.info().name, "Slot");
    let mut w = common::window();
    w.trigger("slot");
    assert_eq!(w.active_tool_key(), Some("slot"));
    assert_eq!(w.stack_current("circles"), Some("slot"));
}
