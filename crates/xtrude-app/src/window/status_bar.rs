// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The main window's status bar, toolkit-neutral: a standing text (the tool's
//! hint), timed messages that cover it for a while and then give it back, and
//! a menu entry's description while it is hovered. The shell feeds it the
//! clock and draws [`current_message`] elided to [`message_max_width`].
//!
//! [`current_message`]: StatusBar::current_message
//! [`message_max_width`]: StatusBar::message_max_width

/// The message never takes more than this share of the bar, so the right
/// end stays clear for the value box.
pub const MESSAGE_SHARE: f64 = 0.6;

#[derive(Clone, Debug, Default)]
pub struct StatusBar {
    base: String,
    shown: String,
    /// When the timed message in `shown` gives way (seconds, the shell's
    /// clock).
    until: Option<f64>,
    tip: Option<String>,
}

impl StatusBar {
    /// `showMessage(text, timeout)`: with a timeout the text covers the
    /// standing one for that long; without, it becomes the standing text.
    pub fn show_message(&mut self, text: &str, timeout_ms: u32, now: f64) {
        if timeout_ms > 0 {
            self.shown = text.to_string();
            self.until = Some(now + timeout_ms as f64 / 1000.0);
        } else {
            self.until = None;
            self.base = text.to_string();
            self.shown = self.base.clone();
        }
    }

    /// The window's hint changed: the new standing text, shown at once
    /// unless a timed message is covering it.
    pub fn set_standing(&mut self, text: &str) {
        if self.base == text {
            return;
        }
        self.base = text.to_string();
        if self.until.is_none() {
            self.shown = self.base.clone();
        }
    }

    /// The timer ran out: the standing text comes back.
    pub fn restore(&mut self) {
        self.until = None;
        self.shown = self.base.clone();
    }

    /// Advance the clock.
    pub fn tick(&mut self, now: f64) {
        if self.until.is_some_and(|t| now >= t) {
            self.restore();
        }
    }

    /// How long until the timed message gives way (seconds, never
    /// negative), `None` without one: the shell asks for a frame then, so
    /// the standing text comes back in a window left alone.
    pub fn next_change(&self, now: f64) -> Option<f64> {
        self.until.map(|t| (t - now).max(0.0))
    }

    /// A menu entry's description while it is hovered (`None` when the
    /// pointer leaves the menu).
    pub fn show_tip(&mut self, tip: Option<&str>) {
        match tip.filter(|t| !t.is_empty()) {
            Some(t) => {
                self.until = None;
                self.tip = Some(t.to_string());
            }
            None => self.tip = None,
        }
    }

    pub fn current_message(&self) -> &str {
        self.tip.as_deref().unwrap_or(&self.shown)
    }

    pub fn standing(&self) -> &str {
        &self.base
    }

    /// The widest the message may be on a bar `bar_width` wide.
    pub fn message_max_width(bar_width: f64) -> f64 {
        (bar_width * MESSAGE_SHARE).trunc().max(120.0)
    }
}
