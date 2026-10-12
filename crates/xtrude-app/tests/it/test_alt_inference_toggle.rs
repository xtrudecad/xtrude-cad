// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Alt cycles the linear inferences DURING an operation only, on the press,
//! swallowing the release; the toggle lasts one operation.

use crate::common;

use common::window;
use glam::DVec3;
use xtrude_app::input::{Button, Key, KeyEvent, Modifiers, MouseEvent};
use xtrude_app::tools::line::LineTool;
use xtrude_app::window::hints::{alt_inference_hint, hint_for};
use xtrude_app::window::MainWindow;
use xtrude_core::snap::LinearMode;

fn press(w: &mut MainWindow, key: Key) {
    w.viewport
        .key_press(&KeyEvent::new(key, "", Modifiers::NONE));
}

fn release(w: &mut MainWindow, key: Key) -> bool {
    w.viewport
        .key_release(&KeyEvent::new(key, "", Modifiers::NONE))
}

/// Put the Line mid-operation, the way the first click does.
fn busy(w: &mut MainWindow) {
    w.viewport.tool_mut::<LineTool>().unwrap().start_point = Some(DVec3::ZERO);
    assert!(w.viewport.tool_busy());
}

fn with_line() -> MainWindow {
    let mut w = window();
    w.activate_tool("line");
    busy(&mut w);
    w
}

fn mode(w: &MainWindow) -> LinearMode {
    w.viewport.inference.linear_inference_mode
}

#[test]
fn test_alt_does_nothing_when_no_operation_is_under_way() {
    let mut w = window();
    assert_eq!(mode(&w), LinearMode::All);
    press(&mut w, Key::Alt);
    release(&mut w, Key::Alt);
    assert_eq!(mode(&w), LinearMode::All);
}

#[test]
fn test_alt_cycles_while_a_line_is_being_drawn() {
    let mut w = with_line();
    for expected in [LinearMode::Off, LinearMode::ParallelPerp, LinearMode::All] {
        press(&mut w, Key::Alt);
        release(&mut w, Key::Alt);
        assert_eq!(mode(&w), expected);
    }
}

#[test]
fn test_it_acts_on_the_press_so_the_menu_bar_cannot_steal_it() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    assert_eq!(mode(&w), LinearMode::Off); // already done
}

#[test]
fn test_the_release_is_swallowed_so_the_menu_bar_never_sees_it() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    assert!(release(&mut w, Key::Alt));
}

#[test]
fn test_an_idle_alt_release_is_left_alone_for_the_menus() {
    let mut w = window();
    press(&mut w, Key::Alt);
    assert!(!w.viewport.alt_tap_pending());
}

#[test]
fn test_losing_focus_drops_the_claim_on_the_release() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    w.viewport.focus_out();
    assert!(!w.viewport.alt_tap_pending());
}

fn click(w: &mut MainWindow) {
    w.viewport
        .mouse_press(&MouseEvent::button(5.0, 5.0, Button::Left, true));
}

#[test]
fn test_the_toggle_lasts_one_operation_and_no_longer() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    assert_eq!(mode(&w), LinearMode::Off);
    w.viewport.tool_mut::<LineTool>().unwrap().start_point = None; // the line was finished
    click(&mut w); // the NEXT gesture starts clean
    assert_eq!(mode(&w), LinearMode::All);
}

#[test]
fn test_a_click_mid_operation_does_not_give_the_inferences_back() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    click(&mut w);
    assert_eq!(mode(&w), LinearMode::Off);
}

#[test]
fn test_changing_tool_gives_the_inferences_back() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    assert_eq!(mode(&w), LinearMode::Off);
    w.activate_tool("select");
    assert_eq!(mode(&w), LinearMode::All);
}

#[test]
fn test_escape_gives_the_inferences_back() {
    let mut w = with_line();
    press(&mut w, Key::Alt);
    w.viewport.escape();
    assert_eq!(mode(&w), LinearMode::All);
}

#[test]
fn test_the_status_bar_offers_alt_and_names_the_state() {
    assert!(alt_inference_hint(LinearMode::All).contains("all"));
    assert!(alt_inference_hint(LinearMode::Off).contains("none"));
    assert!(alt_inference_hint(LinearMode::ParallelPerp).contains("parallel"));
    let mut w = window();
    w.activate_tool("line");
    let idle = hint_for(
        Some("line"),
        w.viewport.active_tool(),
        None,
        Some(LinearMode::All),
    );
    busy(&mut w);
    let drawing = hint_for(
        Some("line"),
        w.viewport.active_tool(),
        None,
        Some(LinearMode::Off),
    );
    assert!(
        !idle.contains("Alt"),
        "nothing to offer before the first click"
    );
    assert!(drawing.contains("Alt") && drawing.contains("none"));
}
