// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A drag from the vertex sets the protractor's axis along it; a click with
//! jitter keeps the inferred plane; the tilt is forgotten after the guide is
//! laid.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;

/// A plan at 100 px per metre.
fn vp() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p: DVec3| Some((p.x * 100.0, p.y * 100.0)));
    vp
}

fn ctx(x: f64, y: f64, z: f64) -> ToolContext {
    let mut c = ToolContext::at(v3(x, y, z));
    c.screen = (x * 100.0, y * 100.0);
    c
}

#[test]
fn test_a_drag_from_the_vertex_sets_the_axis_along_it() {
    let mut vp = vp();
    let mut t = ProtractorTool::new();
    t.on_click(&mut vp, &ctx(1., 1., 0.)); // press on the vertex…
    t.on_hover(&mut vp, &ctx(1., 3., 0.)); // …drag 2 m along +Y…
    t.on_release(&mut vp); // …and let go
    assert!((t.p.axis() - DVec3::Y).length() < 1e-6);
    // The guide lands in the plane normal to the drag.
    t.on_click(&mut vp, &ctx(3., 1., 0.)); // base arm along +X
    t.on_click(&mut vp, &ctx(1., 1., 2.)); // 90° up (in the XZ plane)
    let guides = &vp.scene.doc.guides;
    assert_eq!(guides.len(), 1);
    assert!((guides[0].direction.unwrap().z.abs() - 1.0).abs() < 1e-6);
    assert!((guides[0].point - v3(1., 1., 0.)).length() < 1e-6);
}

#[test]
fn test_a_plain_click_keeps_the_inferred_plane() {
    let mut vp = vp();
    let mut t = ProtractorTool::new();
    t.on_click(&mut vp, &ctx(1., 1., 0.));
    t.on_hover(&mut vp, &ctx(1.02, 1.01, 0.)); // 2 px of jitter, not a drag
    t.on_release(&mut vp);
    assert!(t.p.custom_axis.is_none());
    assert!((t.p.axis() - DVec3::Z).length() < 1e-6);
}

#[test]
fn test_the_tilt_is_forgotten_after_the_guide() {
    let mut vp = vp();
    let mut t = ProtractorTool::new();
    t.on_click(&mut vp, &ctx(0., 0., 0.));
    t.on_hover(&mut vp, &ctx(0., 2., 0.));
    t.on_release(&mut vp);
    t.on_click(&mut vp, &ctx(2., 0., 0.));
    t.on_click(&mut vp, &ctx(0., 0., 2.));
    assert!(t.p.custom_axis.is_none()); // the next measurement starts flat
    assert!(!t.p.axis_drag_armed);
}
