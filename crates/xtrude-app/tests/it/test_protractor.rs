// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Angled guide lines — reset after each guide with a hot retype window, 15°
//! ticks near the disc, rise:run slopes, arrow-key plane lock.

use crate::common;
use crate::common::FakeProbe;
use xtrude_core::Axis;

use common::{stub, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_app::viewport::Viewport;

fn ctx(x: f64, y: f64, z: f64) -> ToolContext {
    let mut c = ToolContext::at(v3(x, y, z));
    c.screen = (x * 100.0, y * 100.0);
    c
}

fn click(vp: &mut Viewport, t: &mut ProtractorTool, x: f64, y: f64, z: f64) {
    t.on_click(vp, &ctx(x, y, z));
}

fn hover(vp: &mut Viewport, t: &mut ProtractorTool, x: f64, y: f64, z: f64) {
    t.on_hover(vp, &ctx(x, y, z));
}

fn screen_stub() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|v: DVec3| Some((v.x * 100.0, v.y * 100.0)));
    vp
}

fn guide_dir(vp: &Viewport, i: usize) -> DVec3 {
    vp.scene.doc.guides[i].direction.unwrap()
}

#[test]
fn test_click_angle_creates_guide_and_resets() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 2., 1., 0.);
    click(&mut vp, &mut t, 5., 1., 0.);
    hover(&mut vp, &mut t, 2., 4., 0.);
    click(&mut vp, &mut t, 2., 4., 0.);
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!(g.is_line());
    assert!((g.point - v3(2., 1., 0.)).length() < 1e-9);
    assert!((g.direction.unwrap().y.abs() - 1.0).abs() < 1e-9);
    assert!(t.p.start_point.is_none() && t.p.ref_point.is_none());
    click(&mut vp, &mut t, 10., 0., 0.);
    click(&mut vp, &mut t, 11., 0., 0.);
    hover(&mut vp, &mut t, 11., 1., 0.);
    click(&mut vp, &mut t, 11., 1., 0.);
    assert_eq!(vp.scene.doc.guides.len(), 2);
    assert!((guide_dir(&vp, 1).x.abs() - 0.5f64.sqrt()).abs() < 1e-6);
}

#[test]
fn test_typed_angle_via_value_box_and_undo() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 1., 0., 0.);
    hover(&mut vp, &mut t, 1., 1., 0.);
    assert!(t.on_value(&mut vp, &ToolValue::Num(30.0)));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let d = guide_dir(&vp, 0);
    assert!((d.x - 30f64.to_radians().cos()).abs() < 1e-6);
    assert!((d.y - 30f64.to_radians().sin()).abs() < 1e-6);
    assert!(vp.undo());
    assert!(vp.scene.doc.guides.is_empty());
}

#[test]
fn test_hot_retype_reaims_last_guide() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 1., 0., 0.);
    hover(&mut vp, &mut t, 1., 1., 0.);
    click(&mut vp, &mut t, 1., 1., 0.);
    assert_eq!(vp.scene.doc.guides.len(), 1);
    assert!(t.on_value(&mut vp, &ToolValue::Num(30.0)));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    assert!((guide_dir(&vp, 0).y - 30f64.to_radians().sin()).abs() < 1e-6);
    assert!(t.on_value(&mut vp, &ToolValue::Num(60.0)));
    assert!((guide_dir(&vp, 0).y - 60f64.to_radians().sin()).abs() < 1e-6);
    assert!(t.on_value(&mut vp, &ToolValue::Num(-60.0)));
    assert!((guide_dir(&vp, 0).y + 60f64.to_radians().sin()).abs() < 1e-6);
    assert!(vp.undo());
    assert!((guide_dir(&vp, 0).y - 60f64.to_radians().sin()).abs() < 1e-6);
    click(&mut vp, &mut t, 5., 5., 0.);
    t.on_cancel(&mut vp);
    assert!(!t.on_value(&mut vp, &ToolValue::Num(10.0)));
}

#[test]
fn test_tick_snapping_near_disc_free_far() {
    let mut vp = screen_stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 1., 0., 0.);
    hover(&mut vp, &mut t, 0.4, 0.3, 0.);
    click(&mut vp, &mut t, 0.4, 0.3, 0.);
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let d = guide_dir(&vp, 0);
    assert!((d.y.atan2(d.x).to_degrees() - 30.0).abs() < 1e-6);
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 1., 0., 0.);
    hover(&mut vp, &mut t, 4., 3., 0.);
    click(&mut vp, &mut t, 4., 3., 0.);
    let d = guide_dir(&vp, 1);
    assert!((d.y.atan2(d.x).to_degrees() - 36.9).abs() < 0.05);
}

#[test]
fn test_arrow_keys_lock_the_plane() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    let c = t.wireframe_color().unwrap();
    assert!(c[..3] != [0.0, 0.0, 0.0]);
    assert!(t.on_key(&mut vp, Key::Right, Modifiers::NONE));
    hover(&mut vp, &mut t, 0., 0., 0.);
    assert_eq!(t.p.axis().x, 1.0);
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 0., 1., 0.);
    hover(&mut vp, &mut t, 0., 0., 1.);
    click(&mut vp, &mut t, 0., 0., 1.);
    assert!((guide_dir(&vp, 0).z.abs() - 1.0).abs() < 1e-6);
    assert!(t.on_key(&mut vp, Key::Right, Modifiers::NONE));
    assert!(t.p.axis_pick.is_none());
    assert!(!t.on_key(&mut vp, Key::Down, Modifiers::NONE));
}

#[test]
fn test_guide_on_a_slanted_work_plane() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    t.p.work_plane = Some((v3(0., 0., 0.), v3(0., 1., 0.)));
    click(&mut vp, &mut t, 0., 0., 0.);
    click(&mut vp, &mut t, 1., 0., 0.);
    hover(&mut vp, &mut t, 0., 0., 1.);
    click(&mut vp, &mut t, 0., 0., 1.);
    assert!((guide_dir(&vp, 0).z.abs() - 1.0).abs() < 1e-6);
}

#[test]
fn test_value_box_parses_slope_ratio() {
    let _m = MetreDocument::new();
    let p = |s: &str| parse_value_buffer(s, false);
    let Some(Typed::Ratio(deg)) = p("3:12") else {
        panic!()
    };
    assert!((deg - 3f64.atan2(12.0).to_degrees()).abs() < 1e-9);
    let Some(Typed::Ratio(deg)) = p("1:6") else {
        panic!()
    };
    assert!((deg - 1f64.atan2(6.0).to_degrees()).abs() < 1e-9);
    let Some(Typed::Ratio(deg)) = p("-1:1") else {
        panic!()
    };
    assert!((deg + 45.0).abs() < 1e-9);
    assert_eq!(p("3:0"), None);
    assert_eq!(p("1:2:3"), None);
    assert_eq!(p("2.5"), Some(Typed::Value(2.5)));
}

#[test]
fn test_reload_and_escape_release_the_arrow_lock() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    assert!(t.on_key(&mut vp, Key::Up, Modifiers::NONE));
    assert_eq!(t.p.axis_pick, Some(Axis::Z));
    t.on_cancel(&mut vp);
    assert!(t.p.axis_pick.is_none());
    assert!(t.on_key(&mut vp, Key::Up, Modifiers::NONE));
    t.on_deactivate(&mut vp);
    t.on_activate(&mut vp);
    assert!(t.p.axis_pick.is_none());
}
