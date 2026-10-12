// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A camera drag whose release was lost does not leave the orbit on, and
//! picking a tool ends a camera drag (seen on Windows 11).

use crate::common;

use common::window;
use xtrude_app::input::{Button, MouseEvent};
use xtrude_app::viewport::{NavMode, Viewport};
use xtrude_app::window::MainWindow;

fn win() -> MainWindow {
    let mut w = window();
    w.viewport.resize(800., 600.);
    w.viewport.camera.set_aspect(800, 600);
    w
}

fn pose(vp: &Viewport) -> (i64, i64) {
    let r = |x: f64| (x * 1e9).round() as i64;
    (r(vp.camera.yaw), r(vp.camera.pitch))
}

/// A wheel-press at (100, 100): a camera drag under way.
fn wheel_press(vp: &mut Viewport) {
    vp.mouse_press(&MouseEvent::button(100., 100., Button::Middle, true));
    assert!(vp.camera_dragging());
}

#[test]
fn test_a_lost_release_does_not_leave_the_orbit_on() {
    let mut w = win();
    let vp = &mut w.viewport;
    wheel_press(vp); // …whose release is lost
    let before = pose(vp);
    vp.mouse_move(&MouseEvent::at(180., 140.)); // a plain move, no button held
    assert_eq!(pose(vp), before);
    assert!(!vp.camera_dragging());
}

#[test]
fn test_a_real_drag_still_orbits() {
    let mut w = win();
    let vp = &mut w.viewport;
    wheel_press(vp);
    let before = pose(vp);
    vp.mouse_move(&MouseEvent::drag(180., 140., Button::Middle)); // the wheel held
    assert_ne!(pose(vp), before);
}

#[test]
fn test_picking_a_tool_ends_a_camera_drag() {
    let mut w = win();
    w.viewport.set_nav_mode(Some(NavMode::Orbit));
    w.viewport
        .mouse_press(&MouseEvent::button(100., 100., Button::Left, true));
    assert!(w.viewport.camera_dragging());
    w.activate_tool("paint");
    assert!(w.viewport.nav.mode.is_none() && !w.viewport.camera_dragging());
}
