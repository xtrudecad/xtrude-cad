// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The dwell timer: resting on an encouraging snap for `ENCOURAGE_MS`
//! encourages it when the frame after that time ticks, with no sleep (the
//! clock is passed in).

use std::time::{Duration, Instant};

use crate::common;
use glam::DVec3;
use xtrude_app::input::{Modifiers, ScreenPt};
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::viewport::{Viewport, ENCOURAGE_MS};

const DUE: Duration = Duration::from_millis(ENCOURAGE_MS);

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn hover(vp: &mut Viewport, p: DVec3) {
    let ScreenPt { x, y } = vp.world_to_pixel(p).expect("on screen");
    vp.process_hover(x, y, Modifiers::NONE);
}

/// A 4 × 3 wall seen from the front, the Rectangle tool active.
fn wall(vp: &mut Viewport) {
    vp.resize(1000., 600.);
    vp.camera.set_aspect(1000, 600);
    vp.scene.mesh_mut().add_face(
        &[v(0., 0., 0.), v(4., 0., 0.), v(4., 0., 3.), v(0., 0., 3.)],
        &[],
    );
    vp.scene.version += 1;
    vp.camera.set_view("front");
    vp.camera.target = v(2., 0., 1.5);
    vp.camera.distance = 12.0;
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
}

fn is(p: Option<DVec3>, q: DVec3) -> bool {
    p.is_some_and(|p| (p - q).length() < 1e-6)
}

#[test]
fn test_no_timer_runs_before_a_hover() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    assert_eq!(vp.dwell_due_in(Instant::now()), None);
}

#[test]
fn test_the_timer_is_due_encourage_ms_after_the_hover() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    let before = Instant::now();
    hover(&mut vp, v(4., 0., 3.));
    let after = Instant::now();
    let left = vp.dwell_due_in(before).expect("a timer");
    assert!(left >= DUE && left <= DUE + (after - before), "{left:?}");
    assert_eq!(vp.dwell_due_in(after + DUE), Some(Duration::ZERO));
    assert_eq!(vp.dwell_due_in(after + DUE * 4), Some(Duration::ZERO)); // overdue
}

#[test]
fn test_a_tick_before_the_time_encourages_nothing() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    let before = Instant::now();
    hover(&mut vp, v(4., 0., 3.));
    vp.tick(before + DUE / 2);
    assert!(vp.inference.acquired_point.is_none());
    assert!(vp.inference.encouraged.is_empty());
    assert!(vp.dwell_active()); // still running
}

#[test]
fn test_a_tick_after_the_time_encourages_the_point() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    hover(&mut vp, v(4., 0., 3.));
    vp.tick(Instant::now() + DUE);
    assert!(is(vp.inference.acquired_point, v(4., 0., 3.)));
    assert_eq!(vp.dwell_due_in(Instant::now()), None); // fired once
}

#[test]
fn test_moving_onto_another_point_restarts_the_timer() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    hover(&mut vp, v(4., 0., 3.));
    let first = Instant::now();
    hover(&mut vp, v(0., 0., 3.));
    let left = vp.dwell_due_in(first).expect("a timer");
    assert!(left >= DUE, "{left:?}");
    assert!(is(vp.dwell_point(), v(0., 0., 3.)));
    vp.tick(Instant::now() + DUE);
    assert!(is(vp.inference.acquired_point, v(0., 0., 3.)));
}

#[test]
fn test_resting_on_the_same_point_keeps_the_timer() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    hover(&mut vp, v(4., 0., 3.));
    let now = Instant::now();
    let left = vp.dwell_due_in(now);
    hover(&mut vp, v(4., 0., 3.));
    assert_eq!(vp.dwell_due_in(now), left);
}

#[test]
fn test_hovering_nothing_stops_the_timer() {
    let mut vp = common::sized_viewport(1000., 600.);
    wall(&mut vp);
    hover(&mut vp, v(4., 0., 3.));
    hover(&mut vp, v(2., 0., 1.5)); // the middle of the face
    assert_eq!(vp.dwell_due_in(Instant::now()), None);
    vp.tick(Instant::now() + DUE);
    assert!(vp.inference.acquired_point.is_none());
}

#[test]
fn test_the_window_tick_fires_the_timer() {
    let mut w = common::window();
    wall(&mut w.viewport);
    assert_eq!(w.next_tick(Instant::now()), None);
    hover(&mut w.viewport, v(4., 0., 3.));
    let left = w.next_tick(Instant::now()).expect("a frame asked for");
    assert!(left <= DUE);
    w.tick(Instant::now());
    assert!(w.viewport.inference.acquired_point.is_none());
    w.tick(Instant::now() + DUE);
    assert!(is(w.viewport.inference.acquired_point, v(4., 0., 3.)));
    assert_eq!(w.next_tick(Instant::now()), None); // nothing more to wake for
}
