// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Measure tool pulls a guide off an edge INSIDE a group, read through
//! `pick_edge_any`.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{PickedEdge, Viewport};
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::guide::Guide;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

/// What pick_edge_any hands back for a group's edge.
fn pseudo_edge(a: DVec3, b: DVec3) -> PickedEdge {
    PickedEdge {
        a,
        b,
        edge: None,
        in_group: true,
        context: None,
        group: None,
        soft: false,
        curve: None,
    }
}

/// `pick_edge` sees the loose mesh, `pick_edge_any` reaches inside groups.
fn vp_with(loose: Option<PickedEdge>, grouped: Option<PickedEdge>) -> Viewport {
    let mut vp = stub();
    let l = loose.clone();
    FakeProbe::of(&mut vp).pick_edge(move |_, _| l.clone());
    let any = grouped.or(loose);
    FakeProbe::of(&mut vp).pick_edge_any(move |_, _| any.clone());
    FakeProbe::of(&mut vp).pick_guide(|_, _| None);
    vp
}

fn ctx(x: f64, y: f64, snap: Option<SnapKind>) -> ToolContext {
    let mut c = ToolContext::at(v(x, y));
    c.snap = snap.map(|k| SnapResult::new(v(x, y), k, [0.0, 0.0, 0.0]));
    c
}

#[test]
fn test_a_group_edge_starts_guide_mode() {
    let mut vp = vp_with(None, Some(pseudo_edge(v(0., 0.), v(4., 0.))));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0., None));
    assert_eq!(
        tool.source(),
        Some((v(0., 0.), v(4., 0.))),
        "the group's edge must be the guide source"
    );
}

#[test]
fn test_and_the_second_click_places_the_guide() {
    let mut vp = vp_with(None, Some(pseudo_edge(v(0., 0.), v(4., 0.))));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0., None)); // grab the group's edge
    tool.on_click(&mut vp, &ctx(1., 2., None)); // pull 2 m off it
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!(g.is_line());
    assert!((g.point.y - 2.0).abs() < 1e-6);
}

#[test]
fn test_returning_to_the_source_edge_places_a_coincident_guide() {
    let mut vp = vp_with(None, Some(pseudo_edge(v(0., 0.), v(4., 0.))));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0., None));
    tool.on_hover(&mut vp, &ctx(1., 2., None));
    tool.on_hover(&mut vp, &ctx(1., 0., None));
    assert!(!tool.guide_preview_lines().is_empty());
    tool.on_click(&mut vp, &ctx(1., 0., None));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!(g.is_line());
    assert!(g.point.y.abs() < 1e-6);
}

#[test]
fn test_a_coincident_existing_guide_can_be_duplicated() {
    let mut vp = vp_with(None, Some(pseudo_edge(v(0., 0.), v(4., 0.))));
    let existing = Guide::new(v(0., 0.), Some(v(1., 0.)), None);
    vp.scene.doc.guides.push(existing.clone());
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0., None));
    tool.on_click(&mut vp, &ctx(1., 0., None));
    assert_eq!(vp.scene.doc.guides.len(), 2);
    assert_eq!(vp.scene.doc.guides[0], existing);
    assert!(vp.scene.doc.guides[1].point.y.abs() < 1e-6);
}

#[test]
fn test_a_loose_edge_still_works() {
    let edge = pseudo_edge(v(0., 0.), v(4., 0.));
    let mut vp = vp_with(Some(edge.clone()), Some(edge));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0., None));
    assert_eq!(tool.source(), Some((v(0., 0.), v(4., 0.))));
}

#[test]
fn test_an_endpoint_snap_still_measures_instead_of_guiding() {
    let mut vp = vp_with(None, Some(pseudo_edge(v(0., 0.), v(4., 0.))));
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(0., 0., Some(SnapKind::Endpoint)));
    assert!(tool.source().is_none());
}

#[test]
fn test_nothing_under_the_cursor_still_measures() {
    let mut vp = vp_with(None, None);
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 1., None));
    assert!(tool.source().is_none());
    tool.on_click(&mut vp, &ctx(4., 1., None));
    assert!(vp.scene.doc.guides.is_empty());
    assert!(tool.measured.is_some());
}
