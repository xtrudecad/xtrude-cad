// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Screen font metrics for the overlay's text (ascent, descent, advance
//! of the default UI font), so the drawn text and its hit box agree.

use xtrude_core::text3d::{horizontal_advance, vertical_metrics, FONT_PX};

/// Metrics of the UI font at `point_size`, 96 DPI.
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    pub px: f64,
    pub bold: bool,
}

impl FontMetrics {
    pub fn new(point_size: f64, bold: bool) -> FontMetrics {
        FontMetrics {
            px: point_size * 96.0 / 72.0,
            bold,
        }
    }

    fn scale(&self) -> f64 {
        self.px / FONT_PX
    }

    fn vertical(&self) -> (f64, f64, f64) {
        vertical_metrics("", self.bold, false)
            .map(|(a, d, h)| (a * self.scale(), d * self.scale(), h * self.scale()))
            .unwrap_or((self.px * 0.93, self.px * 0.24, self.px * 1.17))
    }

    pub fn horizontal_advance(&self, text: &str) -> f64 {
        let w = horizontal_advance(text, "", self.bold, false);
        if w > 0.0 || text.is_empty() {
            (w * self.scale()).round()
        } else {
            (text.chars().count() as f64 * self.px * 0.6).round()
        }
    }

    pub fn ascent(&self) -> f64 {
        self.vertical().0.round()
    }

    pub fn descent(&self) -> f64 {
        self.vertical().1.round()
    }

    pub fn height(&self) -> f64 {
        self.ascent() + self.descent()
    }
}
