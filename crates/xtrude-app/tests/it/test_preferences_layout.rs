// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Preferences dialog keeps one size and place whatever the page: its
//! pages are a list on the left, stacked top to bottom, that a click
//! switches; on a window too small for it, it shrinks to fit and keeps OK
//! on screen.

use crate::common;
use egui::{Event, Pos2, RawInput, Rect, Vec2};
use xtrude_app::shell::prefs_dialog::{preferences_modal, PrefsFrame, PrefsState};
use xtrude_app::window::preferences::{Preferences, PrefsPage};

/// Runs one frame of the dialog on a `screen`-sized window with `events`.
fn frame(
    ctx: &egui::Context,
    state: &mut PrefsState,
    screen: Vec2,
    events: Vec<Event>,
) -> PrefsFrame {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
        events,
        ..RawInput::default()
    };
    let mut shown = None;
    let mut output = ctx.run_ui(input, |ui| {
        shown = Some(preferences_modal(ui.ctx(), state));
    });
    output.textures_delta.clear();
    shown.expect("the dialog ran")
}

/// A few frames on `page`, so the modal's area has settled; the last one.
fn settled(
    ctx: &egui::Context,
    state: &mut PrefsState,
    screen: Vec2,
    page: PrefsPage,
) -> PrefsFrame {
    state.page = page;
    for _ in 0..3 {
        frame(ctx, state, screen, Vec::new());
    }
    frame(ctx, state, screen, Vec::new())
}

fn state() -> PrefsState {
    let win = common::window();
    PrefsState::new(Preferences::load(&win))
}

const LARGE: Vec2 = egui::vec2(1400.0, 900.0);

#[test]
fn test_opens_on_general() {
    assert_eq!(state().page, PrefsPage::General);
}

#[test]
fn test_same_rect_on_every_page() {
    let ctx = egui::Context::default();
    let mut state = state();
    let first = settled(&ctx, &mut state, LARGE, PrefsPage::General).dialog;
    assert!(first.width() > 0.0 && first.height() > 0.0);
    for page in PrefsPage::ALL {
        let shown = settled(&ctx, &mut state, LARGE, page);
        assert_eq!(
            shown.dialog, first,
            "the dialog moved or resized on {page:?}"
        );
        assert!(shown.close.is_none());
    }
}

#[test]
fn test_page_list_is_vertical_left_of_the_page() {
    let ctx = egui::Context::default();
    let mut state = state();
    let shown = settled(&ctx, &mut state, LARGE, PrefsPage::General);
    assert_eq!(shown.entries.len(), PrefsPage::ALL.len());
    for pair in shown.entries.windows(2) {
        assert_eq!(pair[0].left(), pair[1].left());
        assert!(
            pair[0].bottom() <= pair[1].top(),
            "{pair:?} are not stacked"
        );
    }
    for entry in &shown.entries {
        assert!(entry.right() < shown.page_area.left());
        assert!(shown.dialog.contains_rect(*entry));
    }
    assert!(shown.dialog.contains_rect(shown.page_area));
}

#[test]
fn test_clicking_an_entry_switches_the_page() {
    let ctx = egui::Context::default();
    let mut state = state();
    let shown = settled(&ctx, &mut state, LARGE, PrefsPage::General);
    let at = shown.entries[3].center();
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, &mut state, LARGE, vec![Event::PointerMoved(at)]);
    frame(&ctx, &mut state, LARGE, vec![button(true)]);
    frame(&ctx, &mut state, LARGE, vec![button(false)]);
    assert_eq!(state.page, PrefsPage::ALL[3]);
}

#[test]
fn test_small_window_keeps_the_dialog_and_ok_on_screen() {
    let small = egui::vec2(600.0, 400.0);
    let screen = Rect::from_min_size(Pos2::ZERO, small);
    let ctx = egui::Context::default();
    let mut state = state();
    for page in PrefsPage::ALL {
        let shown = settled(&ctx, &mut state, small, page);
        assert!(
            screen.contains_rect(shown.dialog),
            "{:?} leaves the {small:?} window on {page:?}",
            shown.dialog
        );
        assert!(
            shown.dialog.contains_rect(shown.ok),
            "OK is cut on {page:?}"
        );
    }
}
