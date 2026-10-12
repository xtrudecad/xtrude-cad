// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The mouse navigation styles: in each preset the drags its bindings
//! line names orbit, pan or zoom, and nothing else moves the camera; a
//! right click opens the context menu and a right drag does not; the
//! style round-trips through Preferences.

use crate::common;

use xtrude_app::input::{Button, Input, Modifiers, MouseEvent};
use xtrude_app::viewport::{NavAction, NavMode, NavStyle, Trigger, ViewPrefs, Viewport};
use xtrude_app::window::hints::nav_hint;
use xtrude_app::window::preferences::Preferences;
use xtrude_app::window::MainWindow;
use xtrude_model::settings;

const CTRL_SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: true,
    alt: false,
    meta: false,
};
const ALT_SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: false,
    alt: true,
    meta: false,
};
const CTRL_ALT: Modifiers = Modifiers {
    shift: false,
    ctrl: true,
    alt: true,
    meta: false,
};

fn win(style: NavStyle) -> MainWindow {
    let mut w = common::window();
    w.viewport.resize(800., 600.);
    w.viewport.camera.set_aspect(800, 600);
    w.viewport.prefs.nav_style = style;
    w
}

/// What the camera did between two poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Moved {
    Nothing,
    Orbit,
    Pan,
    Zoom,
}

type Pose = (f64, f64, f64, glam::DVec3);

fn pose(vp: &Viewport) -> Pose {
    let c = &vp.camera;
    (c.yaw, c.pitch, c.distance, c.target)
}

fn moved(a: Pose, b: Pose) -> Moved {
    let turned = (a.0 - b.0).abs() > 1e-9 || (a.1 - b.1).abs() > 1e-9;
    if turned {
        Moved::Orbit
    } else if (a.2 - b.2).abs() > 1e-9 {
        Moved::Zoom
    } else if (a.3 - b.3).length() > 1e-9 {
        Moved::Pan
    } else {
        Moved::Nothing
    }
}

fn expected(a: NavAction) -> Moved {
    match a {
        NavAction::Orbit => Moved::Orbit,
        NavAction::Pan => Moved::Pan,
        NavAction::Zoom => Moved::Zoom,
    }
}

fn held(mut e: MouseEvent, buttons: &[Button]) -> MouseEvent {
    for b in buttons {
        match b {
            Button::Left => e.left = true,
            Button::Middle => e.middle = true,
            Button::Right => e.right = true,
        }
    }
    e
}

/// A drag of `button` with `m` from (300, 300) to (360, 250).
fn button_drag(w: &mut MainWindow, button: Button, m: Modifiers) -> Moved {
    let before = pose(&w.viewport);
    w.mouse_press(&MouseEvent::button(300., 300., button, true).with_modifiers(m));
    for i in 1..=6 {
        let t = f64::from(i) / 6.0;
        w.mouse_move(&MouseEvent::drag(300. + 60. * t, 300. - 50. * t, button).with_modifiers(m));
    }
    w.mouse_release(&MouseEvent::button(360., 250., button, false).with_modifiers(m));
    moved(before, pose(&w.viewport))
}

/// Right pressed, then left, a drag with both held, then both released.
fn chord_drag(w: &mut MainWindow, first: Button) -> Moved {
    let second = if first == Button::Left {
        Button::Right
    } else {
        Button::Left
    };
    let before = pose(&w.viewport);
    w.mouse_press(&MouseEvent::button(300., 300., first, true));
    w.mouse_press(&held(
        MouseEvent::button(300., 300., second, true),
        &[first],
    ));
    for i in 1..=6 {
        let t = f64::from(i) / 6.0;
        let e = held(
            MouseEvent::at(300. + 60. * t, 300. - 50. * t),
            &[first, second],
        );
        w.mouse_move(&e);
    }
    w.mouse_release(&held(
        MouseEvent::button(360., 250., second, false),
        &[first],
    ));
    w.mouse_release(&MouseEvent::button(360., 250., first, false));
    moved(before, pose(&w.viewport))
}

/// Moves with no button held and `m`.
fn hover_drag(w: &mut MainWindow, m: Modifiers) -> Moved {
    let before = pose(&w.viewport);
    for i in 0..=6 {
        let t = f64::from(i) / 6.0;
        w.mouse_move(&MouseEvent::at(300. + 60. * t, 300. - 50. * t).with_modifiers(m));
    }
    // letting go of the keys ends it
    w.mouse_move(&MouseEvent::at(360., 250.));
    let after = pose(&w.viewport);
    w.mouse_move(&MouseEvent::at(400., 200.));
    assert_eq!(
        pose(&w.viewport),
        after,
        "a plain move after it moves nothing"
    );
    moved(before, after)
}

fn run(w: &mut MainWindow, trigger: Trigger, m: Modifiers) -> Moved {
    match trigger {
        Trigger::Drag(b) => button_drag(w, b, m),
        Trigger::LeftRightChord => chord_drag(w, Button::Right),
        Trigger::Move => hover_drag(w, m),
    }
}

fn mods(shift: bool, ctrl: bool, alt: bool) -> Modifiers {
    Modifiers {
        shift,
        ctrl,
        alt,
        meta: false,
    }
}

#[test]
fn test_every_binding_of_every_style_moves_the_camera_as_it_says() {
    for style in NavStyle::ALL {
        for b in style.bindings() {
            let mut w = win(style);
            let got = run(&mut w, b.trigger, mods(b.shift, b.ctrl, b.alt));
            assert_eq!(got, expected(b.action), "{style:?} {b:?}");
            assert!(!w.viewport.camera_dragging(), "{style:?} {b:?}");
        }
    }
}

type Gesture = dyn Fn(&mut MainWindow) -> Moved;

/// Each preset as its bindings line says, spelled out, and the drags it
/// leaves alone.
#[test]
fn test_the_presets_bind_what_their_names_say() {
    use Button::{Left, Middle, Right};
    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    let cases: &[(NavStyle, &Gesture, Moved)] = &[
        (
            NavStyle::Default,
            &|w| button_drag(w, Middle, NONE),
            Moved::Orbit,
        ),
        (
            NavStyle::Default,
            &|w| button_drag(w, Middle, SHIFT),
            Moved::Pan,
        ),
        (
            NavStyle::Default,
            &|w| button_drag(w, Right, NONE),
            Moved::Nothing,
        ),
        (
            NavStyle::Default,
            &|w| hover_drag(w, Modifiers::ALT),
            Moved::Nothing,
        ),
        (
            NavStyle::MiddlePans,
            &|w| button_drag(w, Middle, NONE),
            Moved::Pan,
        ),
        (
            NavStyle::MiddlePans,
            &|w| button_drag(w, Middle, SHIFT),
            Moved::Orbit,
        ),
        (
            NavStyle::Chord,
            &|w| button_drag(w, Middle, NONE),
            Moved::Orbit,
        ),
        (
            NavStyle::Chord,
            &|w| button_drag(w, Middle, SHIFT),
            Moved::Pan,
        ),
        (NavStyle::Chord, &|w| chord_drag(w, Right), Moved::Pan),
        (NavStyle::Chord, &|w| chord_drag(w, Left), Moved::Pan),
        (NavStyle::Default, &|w| chord_drag(w, Right), Moved::Nothing),
        (
            NavStyle::RightButton,
            &|w| button_drag(w, Right, NONE),
            Moved::Orbit,
        ),
        (
            NavStyle::RightButton,
            &|w| button_drag(w, Right, SHIFT),
            Moved::Pan,
        ),
        (
            NavStyle::RightButton,
            &|w| button_drag(w, Middle, NONE),
            Moved::Orbit,
        ),
        (
            NavStyle::RightButton,
            &|w| button_drag(w, Left, NONE),
            Moved::Nothing,
        ),
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, Modifiers::ALT),
            Moved::Orbit,
        ),
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, ALT_SHIFT),
            Moved::Pan,
        ),
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, CTRL_ALT),
            Moved::Zoom,
        ),
        // Shift and Ctrl keep their drawing meaning
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, SHIFT),
            Moved::Nothing,
        ),
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, CTRL_SHIFT),
            Moved::Nothing,
        ),
        (NavStyle::Touchpad, &|w| hover_drag(w, NONE), Moved::Nothing),
        (
            NavStyle::Touchpad,
            &|w| hover_drag(w, Modifiers::CTRL),
            Moved::Nothing,
        ),
        (
            NavStyle::Touchpad,
            &|w| button_drag(w, Middle, NONE),
            Moved::Orbit,
        ),
    ];
    for (i, (style, gesture, want)) in cases.iter().enumerate() {
        let mut w = win(*style);
        assert_eq!(gesture(&mut w), *want, "case {i}: {style:?}");
    }
}

#[test]
fn test_the_camera_modes_keep_the_left_button_in_every_style() {
    for style in NavStyle::ALL {
        for (mode, want) in [
            (NavMode::Orbit, Moved::Orbit),
            (NavMode::Pan, Moved::Pan),
            (NavMode::Zoom, Moved::Zoom),
        ] {
            let mut w = win(style);
            w.viewport.set_nav_mode(Some(mode));
            let got = button_drag(&mut w, Button::Left, Modifiers::NONE);
            assert_eq!(got, want, "{style:?} {mode:?}");
        }
    }
}

#[test]
fn test_a_right_click_opens_the_context_menu_in_every_style() {
    for style in NavStyle::ALL {
        let mut w = win(style);
        let before = pose(&w.viewport);
        w.mouse_press(&MouseEvent::button(300., 300., Button::Right, true));
        // a little jitter is still a click
        w.mouse_move(&MouseEvent::drag(303., 302., Button::Right));
        w.mouse_release(&MouseEvent::button(303., 302., Button::Right, false));
        assert!(w.take_context_menu_request(), "{style:?}");
        assert!(!w.take_context_menu_request(), "taken once");
        assert_eq!(
            pose(&w.viewport),
            before,
            "{style:?}: a click moves nothing"
        );
    }
}

#[test]
fn test_a_right_drag_does_not_open_the_context_menu() {
    for style in NavStyle::ALL {
        let mut w = win(style);
        button_drag(&mut w, Button::Right, Modifiers::NONE);
        assert!(!w.take_context_menu_request(), "{style:?}");
    }
}

#[test]
fn test_a_long_right_press_is_not_a_click() {
    let mut w = win(NavStyle::Default);
    w.mouse_press(&MouseEvent::button(300., 300., Button::Right, true).at_time(1000));
    w.mouse_release(&MouseEvent::button(300., 300., Button::Right, false).at_time(2000));
    assert!(!w.take_context_menu_request());
}

#[test]
fn test_the_chord_opens_no_menu_and_hands_the_tool_no_stray_release() {
    for first in [Button::Right, Button::Left] {
        let mut w = win(NavStyle::Chord);
        w.activate_tool("select");
        chord_drag(&mut w, first);
        assert!(!w.take_context_menu_request(), "{first:?} first");
        assert!(!w.viewport.camera_dragging());
        // the next left click is an ordinary one again
        let before = pose(&w.viewport);
        common::click(&mut w.viewport, 200., 200., Modifiers::NONE);
        assert_eq!(pose(&w.viewport), before);
    }
}

#[test]
fn test_a_touchpad_move_that_starts_an_orbit_asks_for_the_depth() {
    let mut w = win(NavStyle::Touchpad);
    let alt = Input::Move(MouseEvent::at(300., 300.).with_modifiers(Modifiers::ALT));
    let plain = Input::Move(MouseEvent::at(300., 300.));
    // no frame on screen yet: nothing to read either way
    assert!(!w.viewport.wants_depth_for(&alt));
    w.viewport.frame_data([800, 600], false);
    assert!(w.viewport.wants_depth_for(&alt));
    assert!(!w.viewport.wants_depth_for(&plain));
    w.viewport.prefs.nav_style = NavStyle::Default;
    assert!(!w.viewport.wants_depth_for(&alt));
}

#[test]
fn test_the_style_round_trips_through_preferences() {
    let mut w = common::window();
    let mut dlg = Preferences::load(&w);
    assert_eq!(dlg.nav_style, NavStyle::Default);
    dlg.nav_style = NavStyle::RightButton;
    dlg.accept(&mut w);
    assert_eq!(
        settings::get_str("nav/style").as_deref(),
        Some("right_button")
    );
    assert_eq!(w.viewport.prefs.nav_style, NavStyle::RightButton);
    assert_eq!(Preferences::load(&w).nav_style, NavStyle::RightButton);
    assert_eq!(ViewPrefs::load().nav_style, NavStyle::RightButton);
    for style in NavStyle::ALL {
        assert_eq!(NavStyle::from_key(style.key()), style);
    }
}

#[test]
fn test_an_unknown_stored_style_loads_as_the_default() {
    let w = common::window();
    settings::set("nav/style", serde_json::json!("spacemouse"));
    assert_eq!(ViewPrefs::load().nav_style, NavStyle::Default);
    assert_eq!(Preferences::load(&w).nav_style, NavStyle::Default);
}

#[test]
fn test_the_cheat_sheet_and_the_orbit_hint_follow_the_style() {
    let sheet = |s: NavStyle| s.cheat_sheet();
    assert_eq!(
        sheet(NavStyle::Default),
        vec![
            ("Orbit".to_string(), "Middle-drag".to_string()),
            ("Pan".to_string(), "Shift + middle-drag".to_string()),
            ("Zoom".to_string(), "Wheel".to_string()),
            ("Context menu".to_string(), "Right-click".to_string()),
        ]
    );
    assert_eq!(sheet(NavStyle::MiddlePans)[0].1, "Shift + middle-drag");
    assert_eq!(sheet(NavStyle::MiddlePans)[1].1, "Middle-drag");
    assert_eq!(
        sheet(NavStyle::Chord)[1].1,
        "Left + right drag, or Shift + middle-drag"
    );
    assert_eq!(
        sheet(NavStyle::RightButton)[0].1,
        "Right-drag, or middle-drag"
    );
    assert_eq!(
        sheet(NavStyle::RightButton)[3].1,
        "Right-click without dragging"
    );
    assert_eq!(sheet(NavStyle::Touchpad)[0].1, "Alt + move, or middle-drag");
    assert_eq!(
        sheet(NavStyle::Touchpad)[1].1,
        "Alt + Shift + move, or Shift + middle-drag"
    );
    assert_eq!(
        sheet(NavStyle::Touchpad)[2].1,
        "Wheel, or Ctrl + Alt + move"
    );

    let orbit = |s: NavStyle| nav_hint(NavMode::Orbit, s);
    assert_eq!(
        orbit(NavStyle::Default),
        "Drag to orbit. Shift = pan. Wheel = zoom. The middle button orbits from any tool."
    );
    assert!(
        orbit(NavStyle::MiddlePans).ends_with("Shift + the middle button orbits from any tool.")
    );
    assert!(orbit(NavStyle::RightButton).ends_with("The right button orbits from any tool."));
    assert!(orbit(NavStyle::Touchpad).ends_with("Alt + a move orbits from any tool."));
    for style in NavStyle::ALL {
        assert!(!orbit(style).contains("  "), "{style:?}");
    }
}

#[test]
fn test_a_touchpad_alt_orbit_is_not_an_alt_tap() {
    use xtrude_app::input::{Key, KeyEvent};
    use xtrude_app::tools::paint::PaintTool;
    use xtrude_app::tools::{paint_sample_armed, set_paint_sample_armed};
    set_paint_sample_armed(false);
    let mut vp = common::sized_viewport(800., 600.);
    vp.prefs.nav_style = NavStyle::Touchpad;
    vp.set_active_tool(Some(Box::new(PaintTool::new())));
    let alt = |vp: &mut Viewport, press: bool| {
        let m = if press {
            Modifiers::ALT
        } else {
            Modifiers::NONE
        };
        vp.app_key_event(&KeyEvent::new(Key::Alt, "", m), press);
    };
    alt(&mut vp, true);
    let before = pose(&vp);
    for i in 0..=6 {
        let x = 300. + 10. * f64::from(i);
        vp.mouse_move(&MouseEvent::at(x, 300.).with_modifiers(Modifiers::ALT));
    }
    assert_eq!(moved(before, pose(&vp)), Moved::Orbit);
    alt(&mut vp, false);
    assert!(
        !paint_sample_armed(),
        "the orbit's Alt release toggled the eyedropper"
    );
    // a tap with no orbit still toggles it
    alt(&mut vp, true);
    alt(&mut vp, false);
    assert!(paint_sample_armed());
    set_paint_sample_armed(false);
}

/// A chord whose last release was lost: the next ordinary click of that
/// button still reaches the tool in full.
#[test]
fn test_a_lost_release_after_a_chord_swallows_nothing_later() {
    for lost in [Button::Left, Button::Right] {
        let mut w = win(NavStyle::Chord);
        // Zoom Window: a left press and release opens and closes a box
        w.viewport.set_nav_mode(Some(NavMode::ZoomWindow));
        w.mouse_press(&MouseEvent::button(300., 300., Button::Right, true));
        w.mouse_press(&held(
            MouseEvent::button(300., 300., Button::Left, true),
            &[Button::Right],
        ));
        let other = if lost == Button::Left {
            Button::Right
        } else {
            Button::Left
        };
        // the other button lets go first, ending the chord; `lost`'s
        // release never comes
        w.mouse_release(&held(MouseEvent::button(300., 300., other, false), &[lost]));
        assert!(!w.viewport.camera_dragging());
        // an ordinary press and release of `lost`
        w.mouse_press(&MouseEvent::button(200., 200., lost, true));
        w.mouse_release(&MouseEvent::button(200., 200., lost, false));
        if lost == Button::Right {
            assert!(w.take_context_menu_request(), "{lost:?}");
        } else {
            assert!(w.viewport.zoom_box().is_none(), "{lost:?}");
        }
    }
}

/// Line mid-operation in a window with `style`.
fn drawing(style: NavStyle) -> MainWindow {
    let mut w = win(style);
    w.activate_tool("line");
    w.viewport
        .tool_mut::<xtrude_app::tools::line::LineTool>()
        .unwrap()
        .start_point = Some(glam::DVec3::ZERO);
    assert!(w.viewport.tool_busy());
    w
}

fn alt_key(w: &mut MainWindow, press: bool) -> bool {
    use xtrude_app::input::{Key, KeyEvent};
    let ev = KeyEvent::new(Key::Alt, "", Modifiers::NONE);
    if press {
        w.viewport.key_press(&ev);
        false
    } else {
        w.viewport.key_release(&ev)
    }
}

#[test]
fn test_touchpad_alt_cycles_the_inferences_on_a_clean_taps_release() {
    use xtrude_core::snap::LinearMode;
    let mut w = drawing(NavStyle::Touchpad);
    let mode = |w: &MainWindow| w.viewport.inference.linear_inference_mode;
    alt_key(&mut w, true);
    assert_eq!(mode(&w), LinearMode::All, "not on the press");
    assert!(alt_key(&mut w, false), "the release is swallowed");
    assert_eq!(mode(&w), LinearMode::Off, "on the release");
    // an Alt held to orbit cycles nothing, and its release is swallowed
    alt_key(&mut w, true);
    let before = pose(&w.viewport);
    for i in 0..=6 {
        let x = 300. + 10. * f64::from(i);
        w.mouse_move(&MouseEvent::at(x, 300.).with_modifiers(Modifiers::ALT));
    }
    assert_eq!(moved(before, pose(&w.viewport)), Moved::Orbit);
    assert!(alt_key(&mut w, false));
    assert_eq!(mode(&w), LinearMode::Off);
    // Alt + click is no tap either
    alt_key(&mut w, true);
    common::click(&mut w.viewport, 200., 200., Modifiers::ALT);
    assert!(w.viewport.tool_busy(), "the line goes on");
    alt_key(&mut w, false);
    assert_eq!(mode(&w), LinearMode::Off);
}

#[test]
fn test_the_other_styles_still_cycle_on_the_alt_press() {
    use xtrude_core::snap::LinearMode;
    for style in NavStyle::ALL {
        if style == NavStyle::Touchpad {
            continue;
        }
        let mut w = drawing(style);
        alt_key(&mut w, true);
        assert_eq!(
            w.viewport.inference.linear_inference_mode,
            LinearMode::Off,
            "{style:?}"
        );
    }
}

#[test]
fn test_touchpad_shift_holds_the_inference_lock_while_drawing() {
    use xtrude_app::input::{Key, KeyEvent};
    let mut w = drawing(NavStyle::Touchpad);
    w.viewport.inference.shift_lock = None;
    let before = pose(&w.viewport);
    w.viewport
        .key_press(&KeyEvent::new(Key::Shift, "", Modifiers::SHIFT));
    for i in 0..=6 {
        let x = 300. + 10. * f64::from(i);
        w.mouse_move(&MouseEvent::at(x, 300.).with_modifiers(Modifiers::SHIFT));
    }
    assert_eq!(pose(&w.viewport), before, "Shift + move does not pan");
    assert!(!w.viewport.camera_dragging());
    assert_eq!(
        w.viewport.input.last_mouse_pos.map(|p| p.x),
        Some(360.),
        "the moves reach the tool"
    );
}
