// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Push/Pull stops level with a guide point or guide line of the Measure tool.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{click, v3, window};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::measure::{MeasureMode, MeasureTool};
use xtrude_app::tools::pushpull::{InferenceKind, PushPullTool};
use xtrude_app::tools::{Tool, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_app::window::MainWindow;
use xtrude_model::guide::Guide;

fn setup() -> MainWindow {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1400., 800.);
    vp.camera.set_aspect(1400, 800);
    let c = &mut vp.camera;
    c.target = v3(2., 2., 1.5);
    c.distance = 16.0;
    c.pitch = 25f64.to_radians();
    c.yaw = (-50f64).to_radians();
    vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
        &[],
    );
    vp.scene.version += 1;
    w
}

fn hover(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    vp.process_hover(x, y, Modifiers::NONE);
}

fn click_at(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    click(vp, x, y, Modifiers::NONE);
}

fn start_push(w: &mut MainWindow) {
    w.activate_tool("pushpull");
    let vp = &mut w.viewport;
    let ScreenPt { x: cx, y: cy } = vp.world_to_pixel(v3(2., 2., 0.)).unwrap();
    hover(vp, cx, cy);
    click_at(vp, cx, cy);
    assert!(vp.tool::<PushPullTool>().unwrap().dragging());
}

/// The Measure tool's own way: click a corner, aim up the blue axis, type.
fn measure_guide_point(w: &mut MainWindow, height: f64) -> DVec3 {
    w.activate_tool("measure");
    let vp = &mut w.viewport;
    vp.tool_mut::<MeasureTool>().unwrap().mode = MeasureMode::Point;
    let ScreenPt { x: x0, y: y0 } = vp.world_to_pixel(v3(4., 4., 0.)).unwrap();
    hover(vp, x0, y0);
    click_at(vp, x0, y0);
    let ScreenPt { x: x1, y: y1 } = vp.world_to_pixel(v3(4., 4., 2.)).unwrap();
    hover(vp, x1, y1);
    let took = vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(height)));
    assert_eq!(took, Some(true));
    let points: Vec<&Guide> = vp
        .scene
        .doc
        .guides
        .iter()
        .filter(|g| g.direction.is_none())
        .collect();
    assert_eq!(points.len(), 1);
    points[0].point
}

fn push_tool(vp: &Viewport) -> &PushPullTool {
    vp.tool::<PushPullTool>().unwrap()
}

#[test]
fn test_push_stops_level_with_a_measure_tool_guide_point() {
    let mut w = setup();
    let guide = measure_guide_point(&mut w, 2.5);
    start_push(&mut w);
    let vp = &mut w.viewport;
    let ScreenPt { x: gx, y: gy } = vp.world_to_pixel(guide).unwrap();
    hover(vp, gx + 2., gy + 1.); // a hair off the guide
    let t = push_tool(vp);
    assert_eq!(t.inference_kind, Some(InferenceKind::GuidePoint));
    assert!((t.pp.extrusion - guide.z).abs() < 1e-9);
    let marker = t.inference_marker();
    assert!(marker.is_some_and(|m| m.1 == InferenceKind::GuidePoint));
    click_at(vp, gx + 2., gy + 1.); // commit
    let m = vp.scene.mesh();
    assert!(
        m.face_ids().iter().any(|&f| m
            .face_positions(f)
            .iter()
            .all(|p| (p.z - guide.z).abs() < 1e-9)),
        "no cap at the guide's height"
    );
}

#[test]
fn test_push_stops_level_with_a_guide_line() {
    let mut w = setup();
    // the kind of line the Measure tool pulls off an edge: along X, 3 m up
    w.viewport
        .scene
        .doc
        .guides
        .push(Guide::new(v3(0., 6., 3.), Some(DVec3::X), None));
    start_push(&mut w);
    let vp = &mut w.viewport;
    let ScreenPt { x: lx, y: ly } = vp.world_to_pixel(v3(2., 6., 3.)).unwrap();
    hover(vp, lx + 1., ly + 2.);
    let t = push_tool(vp);
    assert_eq!(t.inference_kind, Some(InferenceKind::GuideLine));
    assert!((t.pp.extrusion - 3.0).abs() < 1e-9);
}

#[test]
fn test_without_the_guide_the_push_follows_the_cursor() {
    let mut w = setup();
    start_push(&mut w);
    let vp = &mut w.viewport;
    let ScreenPt { x: gx, y: gy } = vp.world_to_pixel(v3(4., 4., 2.5)).unwrap();
    hover(vp, gx + 2., gy + 1.);
    let t = push_tool(vp);
    assert_ne!(t.inference_kind, Some(InferenceKind::GuidePoint));
    assert!((t.pp.extrusion - 2.5).abs() > 1e-6); // nothing to lock on
}
