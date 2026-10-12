// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The guide the Measure tool or the Protractor is about to leave shows before the
//! click — also in perspective, where a guide has one end behind the eye.

use crate::common;

use common::{sized_viewport, stub, v3};
use xtrude_app::tools::measure::{MeasureMode, MeasureTool};
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_model::guide::Guide;

fn ctx(x: f64, y: f64) -> ToolContext {
    let mut c = ToolContext::at(v3(x, y, 0.));
    c.screen = (x * 100.0, y * 100.0);
    c
}

#[test]
fn test_the_protractor_shows_the_guide_before_the_click() {
    let mut vp = stub();
    let mut t = ProtractorTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(2., 1.)); // vertex
    assert!(t.guide_preview_lines().is_empty());
    t.on_click(&mut vp, &ctx(5., 1.)); // base arm = +X
    t.on_hover(&mut vp, &ctx(2., 4.)); // sweeping to 90°
    let lines = t.guide_preview_lines();
    assert_eq!(lines.len(), 1);
    let (a, b) = lines[0];
    let d = (b - a).normalize();
    assert!(d.x.abs() < 1e-6 && (d.y.abs() - 1.0).abs() < 1e-6);
    assert!((a - b).length() > 1000.0); // a guide, not a stub
    t.guides = false; // Ctrl: measure only
    assert!(t.guide_preview_lines().is_empty());
}

#[test]
fn test_the_measure_tool_shows_the_parallel_guide_before_the_click() {
    let mut vp = stub();
    let mut t = MeasureTool::new();
    t.on_activate(&mut vp);
    t.start_point = Some(v3(0., 0., 0.));
    let g = Guide::new(v3(0., 0., 0.), Some(v3(1., 0., 0.)), None); // pulled off X
    t.set_source(Some((g.a(), g.b())));
    t.hover_point = Some(v3(3., 2., 0.));
    let lines = t.guide_preview_lines();
    assert_eq!(lines.len(), 1);
    let (a, b) = lines[0];
    assert!((a.y - 2.0).abs() < 1e-6 && (b.y - 2.0).abs() < 1e-6);
    t.mode = MeasureMode::Measure;
    t.set_source(None);
    assert!(t.guide_preview_lines().is_empty());
}

#[test]
fn test_a_guide_through_the_eye_still_reaches_the_screen() {
    let mut vp = sized_viewport(800., 600.);
    let cam = &mut vp.camera;
    cam.target = v3(0., 0., 0.);
    cam.yaw = (-60f64).to_radians();
    cam.pitch = 20f64.to_radians();
    cam.distance = 10.0;
    // A guide along the sight line, from far behind the eye to far ahead.
    let f = vp.camera.forward();
    let g = Guide::new(vp.camera.target + v3(0.5, 0.5, 0.), Some(f), None);
    let (a, b) = g.segment();
    assert!(vp.world_to_pixel(a).is_none()); // one end behind the eye
    let q = vp.segment_to_pixels(a, b).expect("clipped to the screen");
    for (x, y) in [q.0, q.1] {
        assert!((-65.0..=865.0).contains(&x) && (-65.0..=665.0).contains(&y));
    }
}

#[test]
fn test_clipping_a_guide_through_the_eye_keeps_positive_clip_w() {
    let mut vp = sized_viewport(800., 600.);
    let cam = &mut vp.camera;
    cam.target = v3(0., 0., 0.);
    cam.yaw = (-180f64).to_radians();
    cam.pitch = (-80f64).to_radians();
    cam.distance = 10.0;
    let guide = Guide::new(vp.camera.eye(), Some(vp.camera.forward()), None);
    let (a, b) = guide.segment();
    let q = vp.segment_to_pixels(a, b).expect("clipped");
    assert!([q.0 .0, q.0 .1, q.1 .0, q.1 .1]
        .iter()
        .all(|v| v.is_finite()));
}
