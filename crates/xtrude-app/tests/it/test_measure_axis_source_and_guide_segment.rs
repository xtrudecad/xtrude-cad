// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A model AXIS is a guide source; from a VERTEX the Measure tool leaves a guide POINT
//! with its segment back to the start, drawn in dashes and kept by the document.

use crate::common;
use crate::common::FakeProbe;
use xtrude_core::Axis;

use common::{sized_viewport, stub, v3, TmpDir};
use glam::DVec3;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::guide::Guide;
use xtrude_render::lines::guide_vertices;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn vp_with(axis: Option<Axis>) -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    FakeProbe::of(&mut vp).pick_guide(|_, _| None);
    FakeProbe::of(&mut vp).pick_axis(move |_, _| axis);
    vp
}

fn ctx(x: f64, y: f64, kind: Option<SnapKind>) -> ToolContext {
    let mut c = ToolContext::at(v(x, y));
    c.screen = (x * 100.0, y * 100.0);
    c.snap = kind.map(|k| SnapResult::new(v(x, y), k, [0.0, 0.0, 0.0]));
    c
}

fn flashed(vp: &mut Viewport) -> Vec<String> {
    vp.outbox
        .events
        .drain(..)
        .filter_map(|e| match e {
            xtrude_app::viewport::ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

// ---- 1. an axis is a guide source ---------------------------------------

#[test]
fn test_a_guide_pulled_from_the_red_axis_is_parallel_at_the_typed_distance() {
    let mut vp = vp_with(Some(Axis::X));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(3., 0.02, None)); // on the red axis
    assert!(tool.source().is_some());
    assert!((tool.start_point.unwrap() - v(3., 0.)).length() < 1e-6); // the foot
    tool.on_hover(&mut vp, &ctx(3., 1.2, None)); // pulling toward +Y
    assert!(tool.on_value(&mut vp, &ToolValue::Num(5.0)));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!(g.is_line() && (g.direction.unwrap().x.abs() - 1.0).abs() < 1e-6);
    assert!((g.point.y - 5.0).abs() < 1e-6 && (g.point.x - 3.0).abs() < 1e-6);
}

#[test]
fn test_the_second_click_places_the_axis_guide_too() {
    let mut vp = vp_with(Some(Axis::Y));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(0.01, 2., None)); // on the green axis
    tool.on_click(&mut vp, &ctx(20., 2., None)); // 20 m off it
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!((g.direction.unwrap().y.abs() - 1.0).abs() < 1e-6 && (g.point.x - 20.0).abs() < 1e-6);
}

#[test]
fn test_the_real_viewport_finds_the_axis_under_the_cursor() {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = DVec3::ZERO;
    vp.camera.distance = 12.0;
    let px = vp.world_to_pixel(v(3., 0.)).unwrap();
    assert_eq!(vp.pick_axis(px.x, px.y + 2.0), Some(Axis::X));
    let py = vp.world_to_pixel(v(0., 2.5)).unwrap();
    assert_eq!(vp.pick_axis(py.x - 2.0, py.y), Some(Axis::Y));
    let far = vp.world_to_pixel(v(3., 3.)).unwrap();
    assert_eq!(vp.pick_axis(far.x, far.y), None);
}

// ---- 2. a guide point with its segment -----------------------------------

#[test]
fn test_from_a_vertex_the_second_click_leaves_a_guide_point_with_its_segment() {
    let mut vp = vp_with(None);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 1., Some(SnapKind::Endpoint)));
    tool.on_click(&mut vp, &ctx(3., 1., None)); // free spot
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = vp.scene.doc.guides[0].clone();
    assert!(!g.is_line());
    assert!((g.point - v(3., 1.)).length() < 1e-6);
    assert!(g.origin.is_some_and(|o| (o - v(1., 1.)).length() < 1e-6));
    assert!(flashed(&mut vp).iter().any(|t| t.contains("segment")));
}

#[test]
fn test_a_typed_distance_from_a_vertex_places_the_point_along_the_pull() {
    let mut vp = vp_with(None);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(0., 0., Some(SnapKind::Origin)));
    tool.on_hover(&mut vp, &ctx(4., 0., None));
    assert!(tool.on_value(&mut vp, &ToolValue::Num(1.5)));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!((g.point - v(1.5, 0.)).length() < 1e-6 && g.origin == Some(v(0., 0.)));
}

#[test]
fn test_vertex_to_vertex_only_measures_and_free_space_too() {
    let mut vp = vp_with(None);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 1., Some(SnapKind::Endpoint)));
    tool.on_click(&mut vp, &ctx(3., 1., Some(SnapKind::Endpoint))); // to another corner
    assert!(vp.scene.doc.guides.is_empty());
    tool.on_click(&mut vp, &ctx(5., 5., None)); // free space start
    tool.on_click(&mut vp, &ctx(7., 5., None));
    assert!(vp.scene.doc.guides.is_empty());
    tool.on_click(&mut vp, &ctx(1., 1., Some(SnapKind::Midpoint))); // not a real point
    tool.on_click(&mut vp, &ctx(3., 1., None));
    assert!(vp.scene.doc.guides.is_empty());
}

#[test]
fn test_the_segment_is_drawn_and_survives_the_document() {
    let g = Guide::new(v(3., 1.), None, Some(v(1., 1.)));
    let (_coords, spans) = guide_vertices(std::slice::from_ref(&g), 0.2, &[], None, 0.0);
    assert!(!spans.is_empty() && spans[0].1 >= 2); // dashes along the segment
    assert!(Guide::new(v(3., 1.), Some(v(1., 0.)), Some(v(0., 0.)))
        .origin
        .is_none()); // lines never carry one
    let mut vp = stub();
    vp.scene.doc.guides.push(g);
    let dir = TmpDir::new();
    let path = dir.join("guide.xtr");
    xtrude_io::document::save_scene(&vp.scene, &path).expect("save");
    let mut back = stub();
    xtrude_io::document::load_into(&mut back.scene, &path, None).expect("load");
    assert_eq!(back.scene.doc.guides.len(), 1);
    let b = &back.scene.doc.guides[0];
    assert!(b.origin.is_some_and(|o| (o - v(1., 1.)).length() < 1e-6));
}
