// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Dimension style: the document's dimension look,
//! edited live — decimals, unit, font, colour, ends, baseline step.

use xtrude_model::i18n::tr;

use crate::window::MainWindow;

/// `(key, label)` of the dimension ends.
pub const ENDS: [(&str, &str); 3] = [
    ("arrow", "Arrows"),
    ("tick", "Oblique ticks"),
    ("none", "None"),
];

/// The fields the panel edits.
#[derive(Clone, Debug, PartialEq)]
pub struct DimFields {
    pub decimals: i64,
    pub units: String,
    pub font_size: i64,
    pub color: [u8; 3],
    pub ends: String,
    pub base_step_mm: f64,
}

#[derive(Default)]
pub struct DimensionStyle;

impl DimensionStyle {
    pub fn fields(w: &MainWindow) -> DimFields {
        let s = &w.viewport.scene.dimension_style;
        DimFields {
            decimals: s.decimals,
            units: s.units.clone(),
            font_size: s.font_size,
            color: s.color,
            ends: s.ends.clone(),
            base_step_mm: s.base_step_mm,
        }
    }

    /// The fields written back, the view redrawn.
    pub fn apply(w: &mut MainWindow, f: &DimFields) {
        let s = &mut w.viewport.scene.dimension_style;
        s.decimals = f.decimals.clamp(0, 4);
        s.units = f.units.clone();
        s.font_size = f.font_size.clamp(6, 28);
        s.color = f.color;
        s.ends = f.ends.clone();
        s.base_step_mm = f.base_step_mm.clamp(1.0, 60.0);
        w.viewport.scene.version += 1;
        w.viewport.update();
    }

    pub fn pick_color(w: &mut MainWindow) {
        let mut f = Self::fields(w);
        let cur = f.color.map(|c| c as f64 / 255.0);
        let Some(c) = w.dialogs.get_color(&tr("Dimension color", &[]), cur) else {
            return;
        };
        f.color = c.map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8);
        Self::apply(w, &f);
    }
}
