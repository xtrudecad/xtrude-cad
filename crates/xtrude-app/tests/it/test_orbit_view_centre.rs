// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport tests: an orbit turns about the centre of the view (the
//! camera target), wherever the drag starts and whatever starts it, so
//! a centred model stays centred and the same size; a pan still keeps
//! the grabbed point under the cursor.

use crate::common;

use common::{add_box, drag, screen, v3};
use glam::DVec3;
use xtrude_app::input::{Button, Modifiers, MouseEvent};
use xtrude_app::viewport::{NavAction, NavMode, NavStyle, Trigger};
use xtrude_app::window::MainWindow;

const W: f64 = 800.0;
const H: f64 = 600.0;
const LO: DVec3 = DVec3::new(-1.0, -1.0, 0.0);
const HI: DVec3 = DVec3::new(1.0, 1.0, 2.0);

/// A box seen in perspective, the camera fitted to it.
fn centred_box(style: NavStyle) -> MainWindow {
    let mut w = common::window();
    w.viewport.resize(W, H);
    w.viewport.prefs.nav_style = style;
    add_box(&mut w.viewport, LO, HI);
    let cam = &mut w.viewport.camera;
    cam.set_aspect(W as i32, H as i32);
    cam.perspective = true;
    cam.yaw = (-60f64).to_radians();
    cam.pitch = 25f64.to_radians();
    cam.fit_to(LO, HI, 1.3);
    w
}

/// Where the drags start: on the box's top face near its edge, on the
/// background beside the box, at a corner of the viewport.
fn starts(w: &MainWindow) -> [(f64, f64); 3] {
    [
        screen(&w.viewport, v3(0.9, 0.9, 2.0)),
        (W - 40.0, H / 2.0),
        (5.0, 5.0),
    ]
}

/// The moves of a drag from `from`, by `(dx, dy)`, `m` held.
fn moves(from: (f64, f64), by: (f64, f64)) -> impl Iterator<Item = (f64, f64)> {
    (1..=6).map(move |i| {
        let t = f64::from(i) / 6.0;
        (from.0 + by.0 * t, from.1 + by.1 * t)
    })
}

/// A drag of `button` with `m` from `from`, by `by`.
fn button_drag(w: &mut MainWindow, button: Button, m: Modifiers, from: (f64, f64), by: (f64, f64)) {
    w.mouse_press(&MouseEvent::button(from.0, from.1, button, true).with_modifiers(m));
    for (x, y) in moves(from, by) {
        w.mouse_move(&MouseEvent::drag(x, y, button).with_modifiers(m));
    }
    let to = (from.0 + by.0, from.1 + by.1);
    w.mouse_release(&MouseEvent::button(to.0, to.1, button, false).with_modifiers(m));
}

/// Moves with no button and `m` held from `from`, by `by`, then the keys
/// let go.
fn move_drag(w: &mut MainWindow, m: Modifiers, from: (f64, f64), by: (f64, f64)) {
    w.mouse_move(&MouseEvent::at(from.0, from.1).with_modifiers(m));
    for (x, y) in moves(from, by) {
        w.mouse_move(&MouseEvent::at(x, y).with_modifiers(m));
    }
    w.mouse_move(&MouseEvent::at(from.0 + by.0, from.1 + by.1));
}

/// An orbit drag from a start, by a move.
type Orbit = dyn Fn(&mut MainWindow, (f64, f64), (f64, f64));

/// Several orbit drags by `orbit`, one from each start, alternating
/// directions; after each, the box's centre is still at the screen
/// centre and the target and distance are those of the fit.
fn assert_orbits_about_the_centre(w: &mut MainWindow, what: &str, orbit: &Orbit) {
    let (target, distance) = (w.viewport.camera.target, w.viewport.camera.distance);
    let centre = (LO + HI) * 0.5;
    for round in 0..4 {
        for (i, from) in starts(w).into_iter().enumerate() {
            let sign = if (round + i) % 2 == 0 { 1.0 } else { -1.0 };
            let before = (w.viewport.camera.yaw, w.viewport.camera.pitch);
            orbit(w, from, (sign * 90.0, sign * 35.0));
            let cam = &w.viewport.camera;
            assert!(
                (cam.yaw - before.0).abs() > 1e-6 && (cam.pitch - before.1).abs() > 1e-6,
                "{what}, drag {round}.{i}: it did not orbit"
            );
            assert!(
                (cam.target - target).length() < 1e-9,
                "{what}, drag {round}.{i}: the target moved"
            );
            assert!(
                (cam.distance - distance).abs() < 1e-9,
                "{what}, drag {round}.{i}: the distance changed"
            );
            let (cx, cy) = screen(&w.viewport, centre);
            assert!(
                (cx - W / 2.0).abs() < 1.5 && (cy - H / 2.0).abs() < 1.5,
                "{what}, drag {round}.{i}: the centre is at {cx} {cy}"
            );
            assert!(!w.viewport.camera_dragging(), "{what}");
        }
    }
}

#[test]
fn test_middle_drags_keep_the_model_centred_and_the_same_size() {
    let mut w = centred_box(NavStyle::Default);
    assert_orbits_about_the_centre(&mut w, "middle drag", &|w, from, by| {
        drag(
            &mut w.viewport,
            from.0,
            from.1,
            from.0 + by.0,
            from.1 + by.1,
            Button::Middle,
        )
    });
}

#[test]
fn test_the_orbit_tool_turns_about_the_centre_too() {
    let mut w = centred_box(NavStyle::Default);
    w.viewport.set_nav_mode(Some(NavMode::Orbit));
    assert_orbits_about_the_centre(&mut w, "Orbit tool", &|w, from, by| {
        drag(
            &mut w.viewport,
            from.0,
            from.1,
            from.0 + by.0,
            from.1 + by.1,
            Button::Left,
        )
    });
}

#[test]
fn test_every_style_orbit_binding_turns_about_the_centre() {
    for style in NavStyle::ALL {
        for b in style
            .bindings()
            .iter()
            .filter(|b| b.action == NavAction::Orbit)
        {
            let m = Modifiers {
                shift: b.shift,
                ctrl: b.ctrl,
                alt: b.alt,
                meta: false,
            };
            let what = format!("{style:?} {b:?}");
            let mut w = centred_box(style);
            let trigger = b.trigger;
            assert_orbits_about_the_centre(&mut w, &what, &move |w, from, by| match trigger {
                Trigger::Drag(button) => button_drag(w, button, m, from, by),
                Trigger::Move => move_drag(w, m, from, by),
                Trigger::LeftRightChord => panic!("no style orbits with the chord"),
            });
        }
    }
}

#[test]
fn test_a_pan_keeps_the_grabbed_point_under_the_cursor() {
    let mut w = centred_box(NavStyle::Default);
    let grabbed = v3(0.6, -0.4, 2.0); // on the top face
    let from = screen(&w.viewport, grabbed);
    let p = w.viewport.grab_point_at(from.0, from.1);
    assert!((p - grabbed).length() < 1e-6, "{p:?}");
    let by = (120.0, -70.0);
    button_drag(&mut w, Button::Middle, Modifiers::SHIFT, from, by);
    let (ax, ay) = screen(&w.viewport, grabbed);
    let to = (from.0 + by.0, from.1 + by.1);
    assert!(
        (ax - to.0).abs() < 1.5 && (ay - to.1).abs() < 1.5,
        "{to:?} vs {ax} {ay}"
    );
}

#[test]
fn test_over_empty_sky_a_pan_grabs_the_middle_of_the_model_not_the_origin() {
    // a 10 m box at (500, 500)
    let mut vp = common::viewport();
    vp.resize(W, H);
    add_box(&mut vp, v3(495.0, 495.0, 0.0), v3(505.0, 505.0, 10.0));
    let cam = &mut vp.camera;
    cam.set_aspect(W as i32, H as i32);
    cam.perspective = true;
    cam.target = v3(500.0, 500.0, 5.0);
    cam.distance = 60.0;
    cam.yaw = (-60f64).to_radians();
    cam.pitch = 25f64.to_radians();
    let p = vp.grab_point_at(5.0, 5.0);
    assert!((p - v3(500.0, 500.0, 5.0)).length() < 12.0, "{p:?}");
    assert!(p.length() > 600.0, "it grabbed the world origin");
}

#[test]
fn test_an_empty_scene_grabs_the_target() {
    let mut vp = common::viewport();
    vp.resize(W, H);
    vp.scene.version += 1;
    vp.camera.target = v3(3.0, 4.0, 5.0);
    assert_eq!(vp.grab_point_at(W / 2.0, H / 2.0), v3(3.0, 4.0, 5.0));
}
