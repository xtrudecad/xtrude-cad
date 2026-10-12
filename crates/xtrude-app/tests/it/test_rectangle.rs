// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! On-screen dimensions and typed exact size; Ctrl draws from the centre.

use crate::common;

use std::collections::BTreeSet;

use common::{stub as viewport, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn corner_keys(vp: &Viewport) -> BTreeSet<(i64, i64, i64)> {
    let m = vp.scene.mesh();
    let f = m.face_ids()[0];
    m.face_positions(f)
        .into_iter()
        .map(|p| {
            (
                (p.x * 1000.0).round() as i64,
                (p.y * 1000.0).round() as i64,
                (p.z * 1000.0).round() as i64,
            )
        })
        .collect()
}

fn keys(pts: &[(f64, f64, f64)]) -> BTreeSet<(i64, i64, i64)> {
    pts.iter()
        .map(|p| {
            (
                (p.0 * 1000.0).round() as i64,
                (p.1 * 1000.0).round() as i64,
                (p.2 * 1000.0).round() as i64,
            )
        })
        .collect()
}

#[test]
fn test_dimension_label_tracks_drag() {
    let _m = MetreDocument::new();
    let vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(5.0, 4.0));
    let (text, _) = t.value_label(&vp).unwrap();
    assert_eq!(text, "5.00 × 4.00 m");
}

#[test]
fn test_label_none_before_drag() {
    let vp = viewport();
    assert!(RectangleTool::new().value_label(&vp).is_none());
}

#[test]
fn test_typed_dimensions_build_exact_rectangle() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(5.0, 4.0));
    assert!(t.on_value(&mut vp, &ToolValue::Pair(3.0, 2.0)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert_eq!(
        corner_keys(&vp),
        keys(&[(0., 0., 0.), (3., 0., 0.), (3., 2., 0.), (0., 2., 0.)])
    );
}

#[test]
fn test_typed_dimensions_follow_drag_quadrant() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(-1.0, -1.0));
    assert!(t.on_value(&mut vp, &ToolValue::Pair(3.0, 2.0)));
    assert_eq!(
        corner_keys(&vp),
        keys(&[(0., 0., 0.), (-3., 0., 0.), (-3., -2., 0.), (0., -2., 0.)])
    );
}

#[test]
fn test_typed_dimensions_rejects_non_pair() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(5.0, 4.0));
    assert!(!t.on_value(&mut vp, &ToolValue::Num(3.0)));
    assert!(!t.on_value(&mut vp, &ToolValue::Triple(1.0, 2.0, 3.0)));
    assert_eq!(vp.scene.mesh().faces.len(), 0);
}

#[test]
fn test_typed_single_dimension_builds_square_when_locked() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(5.0, 4.0));
    t.arrow_square_lock = true;
    assert!(t.on_value(&mut vp, &ToolValue::Num(3.0)));
    assert_eq!(
        corner_keys(&vp),
        keys(&[(0., 0., 0.), (3., 0., 0.), (3., 3., 0.), (0., 3., 0.)])
    );
}

#[test]
fn test_ctrl_toggles_centre_mode_and_the_badge() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    assert!(!t.from_center && t.info().icon == Some("rectangle"));
    vp.take_events();
    assert!(t.on_key(&mut vp, Key::Control, Modifiers::NONE));
    assert!(t.from_center && t.info().icon == Some("rectangle_center"));
    assert!(t.on_key(&mut vp, Key::Control, Modifiers::NONE));
    assert!(!t.from_center && t.info().icon == Some("rectangle"));
    let flashes = vp
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, xtrude_app::viewport::ViewportEvent::Flash(..)))
        .count();
    assert_eq!(flashes, 2);
}

#[test]
fn test_from_the_centre_the_second_click_is_a_corner() {
    let _m = MetreDocument::new();
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    t.on_click(&mut vp, &ToolContext::at(v(1.0, 1.0)));
    t.hover_point = Some(v(3.0, 2.0));
    let (text, mid) = t.value_label(&vp).unwrap();
    assert_eq!(text, "4.00 × 2.00 m");
    assert!((mid.unwrap() - v(1.0, 1.0)).length() < 1e-9);
    t.on_click(&mut vp, &ToolContext::at(v(3.0, 2.0)));
    assert_eq!(
        corner_keys(&vp),
        keys(&[(-1., 0., 0.), (3., 0., 0.), (3., 2., 0.), (-1., 2., 0.)])
    );
}

#[test]
fn test_typed_sizes_from_the_centre_are_the_whole_width_and_height() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(5.0, 4.0));
    assert!(t.on_value(&mut vp, &ToolValue::Pair(4.0, 2.0)));
    assert_eq!(
        corner_keys(&vp),
        keys(&[(-2., -1., 0.), (2., -1., 0.), (2., 1., 0.), (-2., 1., 0.)])
    );
}

#[test]
fn test_picking_the_tool_up_again_starts_corner_wise() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    t.on_activate(&mut vp);
    assert!(!t.from_center);
}

#[test]
fn test_square_from_the_centre_is_a_true_square() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.from_center = true;
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(2.0, 1.95));
    let (anchor, far) = t.span(t.hover_point.unwrap());
    let (du, dv) = t.dimensions(anchor, far);
    assert!((du.abs() - dv.abs()).abs() < 1e-9);
    assert!((du.abs() - 4.0).abs() < 1e-9);
    assert!(((anchor + far) * 0.5).length() < 1e-9);
    let (text, mid) = t.value_label(&vp).unwrap();
    assert!(text.ends_with("  (Square)"), "{text}");
    assert!(mid.unwrap().length() < 1e-9);
}

#[test]
fn test_from_the_centre_shows_no_square_cue_when_it_is_not_square() {
    let _m = MetreDocument::new();
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.from_center = true;
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(2.0, 1.5));
    let (text, _) = t.value_label(&vp).unwrap();
    assert_eq!(text, "4.00 × 3.00 m");
    assert!(!text.contains("Square"));
}

#[test]
fn test_committing_a_square_from_the_centre_builds_real_square_corners() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.on_activate(&mut vp);
    t.from_center = true;
    t.start_point = Some(v(0.0, 0.0));
    t.hover_point = Some(v(2.0, 1.95));
    let (anchor, far) = t.span(t.hover_point.unwrap());
    let c = t.corners(anchor, far);
    let sides: Vec<f64> = (0..4).map(|i| (c[(i + 1) % 4] - c[i]).length()).collect();
    let (mx, mn) = (
        sides.iter().cloned().fold(0.0, f64::max),
        sides.iter().cloned().fold(f64::MAX, f64::min),
    );
    assert!(mx - mn < 1e-9);
    assert!(sides.iter().all(|s| (s - 4.0).abs() < 1e-9));
}

fn hover(t: &mut RectangleTool, vp: &mut Viewport, x: f64, y: f64, m: Modifiers) {
    t.on_hover(vp, &ToolContext::at(v(x, y)).with_modifiers(m));
}

#[test]
fn test_shift_locks_square_inference_while_held() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    hover(&mut t, &mut vp, 4.0, 3.9, Modifiers::SHIFT);
    assert!(t.square_locked());
    hover(&mut t, &mut vp, 6.0, 2.0, Modifiers::SHIFT);
    let (_, far) = t.span(t.hover_point.unwrap());
    assert_eq!(t.dimensions(t.start_point.unwrap(), far), (6.0, 6.0));
    hover(&mut t, &mut vp, 6.0, 2.0, Modifiers::NONE);
    assert!(!t.square_locked());
    let (_, far) = t.span(t.hover_point.unwrap());
    assert_eq!(t.dimensions(t.start_point.unwrap(), far), (6.0, 2.0));
}

#[test]
fn test_down_toggles_square_lock_after_square_inference() {
    let mut vp = viewport();
    let mut t = RectangleTool::new();
    t.start_point = Some(v(0.0, 0.0));
    hover(&mut t, &mut vp, 4.0, 3.9, Modifiers::NONE);
    assert!(t.on_key(&mut vp, Key::Down, Modifiers::NONE));
    hover(&mut t, &mut vp, 6.0, 2.0, Modifiers::NONE);
    let (_, far) = t.span(t.hover_point.unwrap());
    assert_eq!(t.dimensions(t.start_point.unwrap(), far), (6.0, 6.0));
    assert!(t.on_key(&mut vp, Key::Down, Modifiers::NONE));
    hover(&mut t, &mut vp, 6.0, 2.0, Modifiers::NONE);
    let (_, far) = t.span(t.hover_point.unwrap());
    assert_eq!(t.dimensions(t.start_point.unwrap(), far), (6.0, 2.0));
}
