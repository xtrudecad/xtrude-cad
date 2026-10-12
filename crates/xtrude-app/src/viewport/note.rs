// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The note beside the pointer: a short message where the eye already is,
//! for what the status bar alone would hide (a refusal, a push that
//! stopped short).

use std::time::{Duration, Instant};

use super::overlay::{qc, Paint, Pen};
use super::Viewport;
use crate::fontmetrics::FontMetrics;

/// The note's text size (px).
const NOTE_SIZE: f32 = 11.0;

/// Where the note sits from the pointer's hot spot (px): below and to the
/// right, clear of the tool cursor.
const NOTE_OFFSET: (f64, f64) = (22.0, 30.0);

/// One note: held while a tool keeps it (`until` is `None`), or shown
/// until `until`.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub until: Option<Instant>,
}

/// The note beside the pointer, if any.
#[derive(Debug, Default)]
pub struct CursorNote {
    pub note: Option<Note>,
}

impl CursorNote {
    /// The note's text while it shows at `now`.
    pub fn text_at(&self, now: Instant) -> Option<&str> {
        let n = self.note.as_ref()?;
        n.until.is_none_or(|u| now < u).then_some(n.text.as_str())
    }
}

impl Viewport {
    /// Show `text` beside the pointer while the tool holds it; `None`
    /// takes a held note away (a timed note stays).
    pub fn hold_note(&mut self, text: Option<&str>) {
        let held = self.note.note.as_ref().is_some_and(|n| n.until.is_none());
        match text {
            Some(t) => {
                let note = Note {
                    text: t.to_string(),
                    until: None,
                };
                if self.note.note.as_ref() != Some(&note) {
                    self.note.note = Some(note);
                    self.update();
                }
            }
            None if held => {
                self.note.note = None;
                self.update();
            }
            None => {}
        }
    }

    /// Show `text` beside the pointer and in the status bar for `msec`.
    pub fn flash_note(&mut self, text: &str, msec: u32) {
        self.flash_status(text, msec);
        self.note.note = Some(Note {
            text: text.to_string(),
            until: Some(Instant::now() + Duration::from_millis(msec as u64)),
        });
        self.update();
    }

    /// The note beside the pointer, while it shows.
    pub fn note_text(&self) -> Option<&str> {
        self.note.text_at(Instant::now())
    }

    /// How long from `now` until a timed note goes (zero when overdue),
    /// `None` while none waits.
    pub fn note_due_in(&self, now: Instant) -> Option<Duration> {
        let until = self.note.note.as_ref()?.until?;
        Some(until.saturating_duration_since(now))
    }

    /// Take a timed note away once its time has come by `now`.
    pub(super) fn expire_note(&mut self, now: Instant) {
        if self.note_due_in(now).is_some_and(|d| d.is_zero()) {
            self.note.note = None;
            self.update();
        }
    }

    /// The note as paint: the text on a light plate, beside the pointer
    /// and kept inside the viewport.
    pub(super) fn overlay_note(&self, out: &mut Vec<Paint>) {
        let (Some(text), Some(at)) = (self.note_text(), self.input.last_mouse_pos) else {
            return;
        };
        let fm = FontMetrics::new(NOTE_SIZE as f64, true);
        let (pad, w, h) = (5.0, fm.horizontal_advance(text), fm.height());
        let mut x = at.x + NOTE_OFFSET.0;
        let mut y = at.y + NOTE_OFFSET.1;
        if x + w + pad > self.width() {
            x = (at.x - NOTE_OFFSET.0 - w).max(pad);
        }
        if y + h + pad > self.height() {
            y = (at.y - NOTE_OFFSET.1 - h).max(pad);
        }
        out.push(Paint::Rect {
            min: (x - pad, y - pad),
            max: (x + w + pad, y + h + pad),
            pen: Some(Pen::solid(qc(176, 48, 48, 255), 1.0)),
            fill: Some(qc(255, 246, 240, 240)),
        });
        out.push(Paint::Text {
            at: (x, y + fm.ascent()),
            text: text.to_string(),
            color: qc(150, 30, 30, 255),
            size: NOTE_SIZE,
            bold: true,
            italic: false,
        });
    }
}
