// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Measure tool pulls a guide from a GUIDE line — how a grid of guides is built.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::guide::Guide;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

/// No mesh edge under the cursor, the guide within threshold.
fn vp_with(guide: &Guide) -> Viewport {
    let mut vp = stub();
    vp.scene.doc.guides.push(guide.clone());
    let id = guide.id;
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    FakeProbe::of(&mut vp).pick_guide(move |_, _| Some(id));
    vp
}

fn ctx(x: f64, y: f64, kind: Option<SnapKind>) -> ToolContext {
    let mut c = ToolContext::at(v(x, y));
    c.screen = (x * 100.0, y * 100.0);
    c.snap = kind.map(|k| SnapResult::new(v(x, y), k, [0.0, 0.0, 0.0]));
    c
}

fn on_edge(x: f64, y: f64) -> ToolContext {
    ctx(x, y, Some(SnapKind::OnEdge))
}

#[test]
fn test_a_guide_pulled_from_a_guide_is_parallel_at_the_offset() {
    let first = Guide::new(v(0., 1.), Some(v(1., 0.)), None); // along X at y = 1
    let mut vp = vp_with(&first);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &on_edge(3., 1.)); // on the guide's body
    assert_eq!(tool.source(), Some((first.a(), first.b())));
    tool.on_click(&mut vp, &ctx(3., 3.5, None)); // 2.5 m away
    assert_eq!(vp.scene.doc.guides.len(), 2);
    let g = &vp.scene.doc.guides[1];
    assert!((g.direction.unwrap().dot(v(1., 0.)).abs() - 1.0).abs() < 1e-6);
    assert!((g.point.y - 3.5).abs() < 1e-6);
}

#[test]
fn test_zero_offset_from_a_guide_can_create_a_coincident_guide() {
    let first = Guide::new(v(0., 1.), Some(v(1., 0.)), None);
    let mut vp = vp_with(&first);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &on_edge(3., 1.));
    tool.on_click(&mut vp, &ctx(3., 1., None));
    assert_eq!(vp.scene.doc.guides.len(), 2);
    assert_eq!(vp.scene.doc.guides[0], first);
    assert!((vp.scene.doc.guides[1].point.y - 1.0).abs() < 1e-6);
}

#[test]
fn test_a_typed_distance_places_the_guide_exactly() {
    let first = Guide::new(v(0., 1.), Some(v(1., 0.)), None);
    let mut vp = vp_with(&first);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &on_edge(3., 1.));
    tool.on_hover(&mut vp, &ctx(3., 2.2, None)); // pulling towards +Y
    assert!(tool.on_value(&mut vp, &ToolValue::Num(2.0)));
    assert_eq!(vp.scene.doc.guides.len(), 2);
    assert!((vp.scene.doc.guides[1].point.y - 3.0).abs() < 1e-6);
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.guides.len(), 1);
}

#[test]
fn test_a_guide_point_is_not_a_source() {
    let pt = Guide::new(v(2., 2.), None, None); // a guide POINT
    let mut vp = vp_with(&pt);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &on_edge(2., 2.));
    assert!(tool.source().is_none()); // measure mode instead
}

#[test]
fn test_the_crossing_of_two_guides_measures_instead_of_pulling() {
    let first = Guide::new(v(0., 1.), Some(v(1., 0.)), None);
    let mut vp = vp_with(&first);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(2., 1., Some(SnapKind::Intersection))); // the green X is a point
    assert!(tool.source().is_none());
}
