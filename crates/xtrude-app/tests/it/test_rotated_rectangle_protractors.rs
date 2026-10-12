// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A protractor at the first corner while the base edge is drawn, a second one
//! square to the base edge while the width and its angle are set; Shift holds
//! either.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::rotated_rectangle::RotatedRectangleTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::units::{set_override, Units};

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn w2p(p: DVec3) -> (f64, f64) {
    (p.x * 100.0, -p.y * 100.0 - p.z * 100.0)
}

/// A plan view: 100 px per metre, looking down.
fn plan() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some(w2p(p)));
    vp
}

fn ctx(p: DVec3, mods: Modifiers) -> ToolContext {
    let mut c = ToolContext::at(p).with_modifiers(mods);
    c.screen = w2p(p);
    c
}

fn plain(p: DVec3) -> ToolContext {
    ctx(p, Modifiers::NONE)
}

fn shift(p: DVec3) -> ToolContext {
    ctx(p, Modifiers::SHIFT)
}

fn started(vp: &mut Viewport) -> RotatedRectangleTool {
    let mut t = RotatedRectangleTool::new();
    t.on_click(vp, &plain(v(0., 0.)));
    t
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-5
}

#[test]
fn test_the_first_protractor_is_drawn_at_the_first_corner() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &plain(v(3., 1.)));
    let segs = tool.rubber_band_lines(&vp);
    assert!(segs.len() > 40); // the edge + rim + ticks
    let radii: std::collections::BTreeSet<i64> = segs[1..49]
        .iter()
        .map(|(a, _)| (a.length() * 1e6).round() as i64)
        .collect();
    assert_eq!(radii, [600_000].into_iter().collect()); // 60 px at 100 px/m
    assert!(tool.value_label(&vp).unwrap().0.contains("18°")); // the edge's direction
}

#[test]
fn test_near_the_rim_the_edge_turns_in_15_degree_steps() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    let a = 37f64.to_radians();
    tool.on_hover(&mut vp, &plain(v(0.5 * a.cos(), 0.5 * a.sin())));
    let d = tool.hover_point.unwrap();
    assert!((d.y.atan2(d.x).to_degrees() - 30.0).abs() < 1e-6);
    assert!((d.length() - 0.5).abs() < 1e-6); // the length stays
    let far = v(3. * a.cos(), 3. * a.sin());
    tool.on_hover(&mut vp, &plain(far));
    assert!(close(tool.hover_point.unwrap(), far)); // far out: free
}

#[test]
fn test_the_base_edge_can_be_typed_with_its_angle() {
    let _m = MetreDocument::new();
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &plain(v(1., 0.)));
    assert_eq!(tool.value_box_caption().as_deref(), Some("Length; angle"));
    assert!(tool.on_value(&mut vp, &ToolValue::Pair(2.0, 90.0)));
    assert!(close(tool.base_point.unwrap(), v(0., 2.)));
    assert_eq!(tool.value_box_caption().as_deref(), Some("Width; angle"));
}

#[test]
fn test_a_typed_length_follows_the_cursor() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &plain(v(3., 4.)));
    assert!(tool.on_value(&mut vp, &ToolValue::Num(10.0)));
    assert!(close(tool.base_point.unwrap(), v(6., 8.)));
}

#[test]
fn test_the_second_protractor_stands_square_to_the_base_edge() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_click(&mut vp, &plain(v(2., 0.)));
    tool.on_hover(&mut vp, &plain(v(1., 1.)));
    let segs = tool.rubber_band_lines(&vp);
    let disc = &segs[4..];
    assert!(!disc.is_empty());
    for (a, b) in disc {
        // every point in the plane x = 2
        assert!((a.x - 2.0).abs() < 1e-6 && (b.x - 2.0).abs() < 1e-6);
    }
}

#[test]
fn test_shift_holds_the_width_angle() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_click(&mut vp, &plain(v(2., 0.)));
    tool.on_hover(&mut vp, &plain(v(1., 3.)));
    assert_eq!(tool.angle, 0.0); // flat on the ground
    tool.on_hover(&mut vp, &shift(v(1., 3.)));
    tool.on_hover(&mut vp, &shift(v3(1., 1., 5.))); // would stand up
    let (w, angle) = tool.width_and_angle(tool.hover_point.unwrap(), tool.locked);
    assert!(angle == 0.0 && (w - 1.0).abs() < 1e-6);
}

#[test]
fn test_an_angle_is_not_a_length_in_a_millimetre_model() {
    // «3;90» in a millimetre document: the bare fields are millimetres.
    set_override(Some(Units::new("mm", 0)));
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &plain(v(1., 0.)));
    let ok = tool.on_value(&mut vp, &ToolValue::Pair(2.0, 90.0 * 0.001));
    set_override(None);
    assert!(ok);
    assert!(close(tool.base_point.unwrap(), v(0., 2.)));
}

/// A perspective eye: the ray runs from `eye` to `aim` whatever the pixel.
fn eye_vp(eye: DVec3, aim: DVec3) -> Viewport {
    let mut vp = stub();
    let d = (aim - eye).normalize();
    FakeProbe::of(&mut vp).pixel_to_ray(move |_, _| Some((eye, d)));
    vp
}

fn snapped(p: DVec3, kind: SnapKind) -> ToolContext {
    let mut c = ToolContext::at(p);
    c.snap = Some(SnapResult::new(p, kind, [0.0, 0.0, 0.0]));
    c
}

#[test]
fn test_the_width_is_read_on_the_second_protractors_plane() {
    let aim = v3(2., -1., 1.); // on the plane x = 2
    let mut vp = eye_vp(v3(5., -4., 3.), aim);
    let mut tool = RotatedRectangleTool::new();
    tool.on_click(&mut vp, &snapped(v(0., 0.), SnapKind::None));
    tool.on_click(&mut vp, &snapped(v(2., 0.), SnapKind::None));
    let ground = v(2. + 3. * 1.5, -1. - 3. * 1.5); // where the ray meets z=0
    tool.on_hover(&mut vp, &snapped(ground, SnapKind::None));
    assert!(close(tool.hover_point.unwrap(), aim));
    let (w, angle) = tool.width_and_angle(tool.hover_point.unwrap(), None);
    assert!((angle - 135.0).abs() < 1e-6); // tilted, not 180°
    assert!((w - 2f64.sqrt()).abs() < 1e-6);
}

#[test]
fn test_a_snapped_point_keeps_the_width_on_it() {
    let mut vp = eye_vp(v3(5., -4., 3.), v3(2., -1., 1.));
    let mut tool = RotatedRectangleTool::new();
    tool.on_click(&mut vp, &snapped(v(0., 0.), SnapKind::None));
    tool.on_click(&mut vp, &snapped(v(2., 0.), SnapKind::None));
    let corner = v(2., 3.);
    tool.on_hover(&mut vp, &snapped(corner, SnapKind::Endpoint));
    assert!(close(tool.hover_point.unwrap(), corner));
}

#[test]
fn test_shift_holds_the_base_edge_direction() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &plain(v(3., 3.))); // 45°
    tool.on_hover(&mut vp, &shift(v(3., 3.))); // held
    tool.on_hover(&mut vp, &shift(v(5., 1.))); // the cursor wanders
    let h = tool.hover_point.unwrap();
    assert!((h.x - h.y).abs() < 1e-6 && h.x > 0.0); // still at 45°
    assert!(tool.on_value(&mut vp, &ToolValue::Num(2.0))); // 2 m along it
    let r = 2.0 / 2f64.sqrt();
    assert!(close(tool.base_point.unwrap(), v(r, r)));
}

#[test]
fn test_the_click_lands_on_the_held_line_and_releasing_frees_it() {
    let mut vp = plan();
    let mut tool = started(&mut vp);
    tool.on_hover(&mut vp, &shift(v(4., 0.))); // held along X
    tool.on_hover(&mut vp, &shift(v(3., 2.)));
    assert!(close(tool.hover_point.unwrap(), v(3., 0.)));
    tool.on_hover(&mut vp, &plain(v(3., 2.))); // Shift up: free
    assert!(close(tool.hover_point.unwrap(), v(3., 2.)));
    tool.on_hover(&mut vp, &shift(v(4., 0.)));
    tool.on_hover(&mut vp, &shift(v(3., 2.)));
    tool.on_click(&mut vp, &shift(v(3., 2.)));
    assert!(close(tool.base_point.unwrap(), v(3., 0.)));
}
