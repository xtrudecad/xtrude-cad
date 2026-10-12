// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The platform clipboard behind `window::clipboard`, on `arboard`:
//! the model clipboard travels as text.

use crate::window::clipboard::{TextClipboard, TextStore};

/// `arboard`'s text clipboard; `None` when the platform has none to open
/// (copies then stay in this window).
pub struct ArboardStore(Option<arboard::Clipboard>);

impl TextStore for ArboardStore {
    fn get_text(&mut self) -> Option<String> {
        self.0.as_mut()?.get_text().ok()
    }

    fn set_text(&mut self, text: String) -> bool {
        let Some(cb) = self.0.as_mut() else {
            return false;
        };
        match cb.set_text(text) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("could not write the system clipboard: {e}");
                false
            }
        }
    }
}

/// The system clipboard the window offers its copies on.
pub fn system_clipboard() -> TextClipboard<ArboardStore> {
    let cb = arboard::Clipboard::new()
        .map_err(|e| eprintln!("no system clipboard: {e}"))
        .ok();
    TextClipboard::new(ArboardStore(cb))
}
