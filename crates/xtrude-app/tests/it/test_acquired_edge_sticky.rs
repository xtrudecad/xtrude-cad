// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An edge hovered while a segment is under way stays acquired after the
//! cursor leaves it (the parallel lock's reference), and is dropped once
//! nothing is drawn.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{sized_viewport, v3};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::ToolContext;
use xtrude_app::viewport::Viewport;

fn vp() -> Viewport {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v3(3., 2., 0.);
    vp.camera.distance = 12.0;
    // An off-axis wall (27°): only the parallel lock draws along it.
    vp.scene
        .mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(4., 2., 0.))
        .unwrap();
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v3(1., 4., 0.))));
    assert!(vp.tool::<LineTool>().unwrap().start_point.is_some()); // under way
    vp
}

fn hover(vp: &mut Viewport, p: DVec3) {
    let ScreenPt { x, y } = vp.world_to_pixel(p).unwrap();
    vp.process_hover(x, y, Modifiers::NONE);
}

#[test]
fn test_the_hovered_edge_survives_the_cursor_leaving_it() {
    let mut vp = vp();
    hover(&mut vp, v3(2., 1., 0.)); // the wall's body
    let h = vp
        .inference
        .hover_edge
        .clone()
        .expect("the wall under the cursor");
    assert_eq!(vp.inference.acquired_edge, Some((h.a, h.b)));
    hover(&mut vp, v3(4., 5.5, 0.)); // empty space, off the wall
    assert!(vp.inference.hover_edge.is_none()); // nothing under the cursor…
    let e = vp
        .inference
        .acquired_edge
        .expect("…but the wall is remembered");
    assert!(e.0.length() < 1e-6 && (e.1 - v3(4., 2., 0.)).length() < 1e-6);
}

#[test]
fn test_the_reference_is_dropped_once_nothing_is_being_drawn() {
    let mut vp = vp();
    hover(&mut vp, v3(2., 1., 0.));
    assert!(vp.inference.acquired_edge.is_some());
    vp.tool_mut::<LineTool>().unwrap().start_point = None; // Esc / finished
    hover(&mut vp, v3(4., 5.5, 0.));
    assert!(vp.inference.acquired_edge.is_none()); // never stale across draws
}
