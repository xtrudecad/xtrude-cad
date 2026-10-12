// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What the keyboard and the pointer did before the event at hand.

use crate::input::{Key, KeyEvent, Modifiers, ScreenPt};

/// The keys held and the pointer's last place: the modifiers, the Alt and
/// Ctrl taps, the last hover and the last double click.
///
/// `alt_tap` is set only between an Alt press the viewport acted on and
/// its release, which it then swallows; `alt_cycle_on_release` while that
/// press still waits for its release to cycle the inferences (a style
/// that navigates with Alt + move) and nothing else used the Alt; `ctrl_alone` only while the Ctrl
/// being held has had no other key with it. Losing the keyboard focus
/// drops every key claim (`focus_out`).
#[derive(Debug, Default)]
pub struct InputState {
    pub(super) alt_tap: bool,
    pub(super) alt_cycle_on_release: bool,
    pub(super) alt_down: bool,
    /// Alt is down and nothing else was pressed since (application-wide).
    pub(super) alt_clean: bool,
    pub(super) ctrl_alone: bool,
    /// The keyboard modifiers held now.
    pub(super) held_modifiers: Modifiers,
    /// Where the pointer last hovered.
    pub last_mouse_pos: Option<ScreenPt>,
    /// The last double click: its time (ms) and position, for the triple
    /// click.
    pub(super) last_double: Option<(u64, (f64, f64))>,
}

impl InputState {
    /// A key press: the modifiers it holds down, and whether a Ctrl is
    /// still pressed alone.
    pub(super) fn key_pressed(&mut self, ev: &KeyEvent) {
        self.held_modifiers = ev.modifiers;
        match ev.key {
            Key::Shift => self.held_modifiers.shift = true,
            Key::Control => self.held_modifiers.ctrl = true,
            Key::Alt => self.held_modifiers.alt = true,
            _ => {}
        }
        if !is_modifier_key(ev.key) {
            self.ctrl_alone = false;
        }
        if ev.key == Key::Control && !ev.auto_repeat {
            self.ctrl_alone = true;
        }
    }

    /// A key release: the modifier it lets go of.
    pub(super) fn key_released(&mut self, ev: &KeyEvent) {
        match ev.key {
            Key::Shift => self.held_modifiers.shift = false,
            Key::Control => self.held_modifiers.ctrl = false,
            Key::Alt => self.held_modifiers.alt = false,
            _ => {}
        }
    }

    /// The keyboard went elsewhere: claims on pending releases lapse.
    pub(super) fn focus_out(&mut self) {
        self.alt_tap = false;
        self.alt_cycle_on_release = false;
        self.alt_down = false;
        self.ctrl_alone = false;
        self.held_modifiers = Modifiers::NONE;
    }
}

pub(super) fn is_modifier_key(k: Key) -> bool {
    matches!(k, Key::Control | Key::Shift | Key::Alt | Key::Meta)
}
