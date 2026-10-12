// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Pushing the top of a box down follows the cursor as smoothly as
//! pulling it up: the walls the push slides along, the edges of the cap
//! where the preview moved it, and corners and edges hidden behind the
//! box give no reference distance.

use crate::common;

use common::{add_box, click, hover, v3, window};
use xtrude_app::input::Modifiers;
use xtrude_app::input::ScreenPt;
use xtrude_app::tools::pushpull::{InferenceKind, PushPullTool};
use xtrude_app::viewport::Viewport;

fn tool(vp: &Viewport) -> &PushPullTool {
    vp.tool::<PushPullTool>().unwrap()
}

/// A 4 × 3 × 2 m box seen from the front and above, Push/Pull locked on
/// the middle of its top; returns the window and that point on screen.
fn box_top_locked() -> (xtrude_app::window::MainWindow, ScreenPt) {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1400., 800.);
    vp.camera.set_aspect(1400, 800);
    let c = &mut vp.camera;
    c.target = v3(2., 1.5, 1.);
    c.distance = 12.0;
    c.pitch = 25f64.to_radians();
    c.yaw = (-70f64).to_radians();
    add_box(vp, v3(0., 0., 0.), v3(4., 3., 2.));
    w.activate_tool("pushpull");
    let vp = &mut w.viewport;
    let at = vp.world_to_pixel(v3(2., 1.5, 2.)).unwrap();
    hover(vp, at.x, at.y);
    click(vp, at.x, at.y, Modifiers::NONE);
    assert!(tool(vp).dragging());
    (w, at)
}

#[test]
fn test_pushing_the_top_down_over_the_front_wall_follows_the_cursor() {
    let (mut w, ScreenPt { x: cx, y: cy }) = box_top_locked();
    let vp = &mut w.viewport;
    // Down the screen in equal steps to where the push axis is 1.8 m
    // down: the cursor crosses the top front edge, then runs over the
    // front wall, past the back bottom edge hidden behind it.
    let ScreenPt { y: end, .. } = vp.world_to_pixel(v3(2., 1.5, 0.2)).unwrap();
    let mut pushes = Vec::new();
    let steps = 24;
    for i in 1..=steps {
        hover(vp, cx, cy + (end - cy) * i as f64 / steps as f64);
        pushes.push(tool(vp).pp.extrusion);
        assert_eq!(tool(vp).inference_kind, None, "{pushes:?}");
    }
    assert!(
        pushes.windows(2).all(|p| p[1] < p[0]),
        "the push must deepen with each step down: {pushes:?}"
    );
    assert!((pushes[steps - 1] + 1.8).abs() < 1e-6, "{pushes:?}");
}

#[test]
fn test_the_moved_cap_edges_give_no_reference_distance() {
    let (mut w, ScreenPt { x: cx, y: cy }) = box_top_locked();
    let vp = &mut w.viewport;
    hover(vp, cx, cy + 50.0);
    let first = tool(vp).pp.extrusion;
    assert!(first < -0.5, "{first}");
    // The preview left the top front edge `first` lower: the cursor on
    // it is no edge of the model once the cap is back in place.
    let moved = vp.world_to_pixel(v3(2., 0., 2. + first)).unwrap();
    hover(vp, moved.x, moved.y);
    assert_eq!(tool(vp).inference_kind, None);
    assert!(
        tool(vp).pp.extrusion < first - 0.1,
        "{first} {}",
        tool(vp).pp.extrusion
    );
}

#[test]
fn test_a_corner_hidden_behind_the_box_gives_no_reference_distance() {
    let (mut w, _) = box_top_locked();
    let vp = &mut w.viewport;
    // The back left bottom corner lies behind the front wall.
    let hidden = vp.world_to_pixel(v3(0., 3., 0.)).unwrap();
    hover(vp, hidden.x, hidden.y);
    assert_eq!(tool(vp).inference_kind, None);
    // The front bottom corner is in sight: it still levels the push.
    let seen = vp.world_to_pixel(v3(0., 0., 0.)).unwrap();
    hover(vp, seen.x, seen.y);
    assert_eq!(tool(vp).inference_kind, Some(InferenceKind::Vertex));
    assert!((tool(vp).pp.extrusion + 2.0).abs() < 1e-9);
}
