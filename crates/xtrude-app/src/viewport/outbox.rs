// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What the viewport hands to the window.

use super::{Cursor, ViewportEvent};

/// The viewport's output to the window: the queued events, the pointer it
/// asks for, and the repaints asked for.
///
/// `events` is oldest first and the window drains it
/// ([`super::Viewport::take_events`]); `status_flash` is the text of the
/// last [`ViewportEvent::Flash`] queued, kept after the drain.
#[derive(Debug, Default)]
pub struct Outbox {
    /// Signals for the window, oldest first.
    pub events: Vec<ViewportEvent>,
    /// The last status-bar flash (text, ms).
    pub status_flash: Option<(String, u32)>,
    /// The pointer the tool or navigation asks for, `None` = the arrow.
    pub cursor: Option<Cursor>,
    pub(super) redraws: u64,
}

impl Outbox {
    /// Queue a status-bar flash.
    pub(super) fn flash(&mut self, text: &str, msec: u32) {
        self.status_flash = Some((text.to_string(), msec));
        self.events
            .push(ViewportEvent::Flash(text.to_string(), msec));
    }
}
