// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Light / dark UI chrome that follows the desktop, or a fixed choice.
//!
//! Window ▸ Preferences ▸ Theme stores `general/theme`: `dark` (the
//! default), `light`, or `system`, which follows the OS colour scheme. Only
//! the chrome changes: the 3D viewport keeps the model's display style.
//! The palettes are plain, neutral grey desktop palettes, toolkit-neutral;
//! the shell turns them into egui visuals.

use xtrude_model::settings::{self, keys};

pub const SYSTEM: &str = "system";
pub const LIGHT: &str = "light";
pub const DARK: &str = "dark";
pub const THEMES: [&str; 3] = [SYSTEM, LIGHT, DARK];

/// The one accent both themes share — selection and the Xtrude CAD blue.
pub const HIGHLIGHT: [u8; 3] = [42, 93, 143];

pub fn saved_theme() -> String {
    match settings::stored(&keys::THEME) {
        Some(m) if THEMES.contains(&m.as_str()) => m,
        _ => DARK.to_string(),
    }
}

pub fn save_theme(mode: &str) {
    let m = if THEMES.contains(&mode) { mode } else { DARK };
    settings::write(&keys::THEME, &m.to_string());
}

/// Which palette a mode resolves to. The system saying nothing (`None`:
/// an old desktop, a bare X session) keeps the dark chrome.
pub fn is_dark(mode: &str, system_dark: Option<bool>) -> bool {
    match mode {
        LIGHT => false,
        DARK => true,
        _ => system_dark != Some(false),
    }
}

/// The palette roles the chrome uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub window: [u8; 3],
    pub window_text: [u8; 3],
    pub base: [u8; 3],
    pub alternate_base: [u8; 3],
    pub placeholder: [u8; 3],
    pub tooltip_base: [u8; 3],
    pub highlight: [u8; 3],
    pub highlighted_text: [u8; 3],
    pub link: [u8; 3],
    pub bright_text: [u8; 3],
    pub light: [u8; 3],
    pub midlight: [u8; 3],
    pub mid: [u8; 3],
    pub dark: [u8; 3],
    pub shadow: [u8; 3],
    pub disabled: [u8; 3],
    /// The value box while a value is typed into it: a fill that stands
    /// out from `base` (the box at rest) and the ink drawn on it.
    pub typing_fill: [u8; 3],
    pub typing_text: [u8; 3],
}

pub fn dark_palette() -> Palette {
    Palette {
        window: [45, 45, 48],
        window_text: [224, 224, 224],
        base: [37, 37, 40],
        alternate_base: [45, 45, 48],
        placeholder: [128, 128, 128],
        tooltip_base: [58, 58, 61],
        highlight: HIGHLIGHT,
        highlighted_text: [255, 255, 255],
        link: [74, 163, 224],
        bright_text: [255, 96, 96],
        light: [0x34, 0x34, 0x34],
        // Hovered and pressed widgets (menu bar entries, buttons) are
        // filled with `midlight` and `mid` over `window`: they step up
        // from it as the light palette's step down.
        midlight: [0x4A, 0x4A, 0x4A],
        mid: [0x58, 0x58, 0x58],
        dark: [0x25, 0x25, 0x25],
        shadow: [0x02, 0x02, 0x02],
        disabled: [128, 128, 128],
        // a muted amber: light ink reads on it
        typing_fill: [122, 92, 28],
        typing_text: [255, 255, 255],
    }
}

pub fn light_palette() -> Palette {
    Palette {
        window: [239, 239, 239],
        window_text: [20, 22, 26],
        base: [255, 255, 255],
        alternate_base: [247, 247, 247],
        placeholder: [120, 124, 130],
        tooltip_base: [255, 255, 225],
        highlight: HIGHLIGHT,
        highlighted_text: [255, 255, 255],
        link: [26, 110, 190],
        bright_text: [200, 30, 30],
        light: [255, 255, 255],
        midlight: [0xCA, 0xCA, 0xCA],
        mid: [0xB8, 0xB8, 0xB8],
        dark: [0x9F, 0x9F, 0x9F],
        shadow: [0x76, 0x76, 0x76],
        disabled: [160, 160, 160],
        typing_fill: [255, 236, 179],
        typing_text: [20, 22, 26],
    }
}

/// HSL lightness: the mean of the largest and smallest channel, 0–255.
pub fn lightness(c: [u8; 3]) -> u8 {
    let max = c.iter().copied().max().unwrap_or(0) as u16;
    let min = c.iter().copied().min().unwrap_or(0) as u16;
    ((max + min) / 2) as u8
}

/// Colours a template may name, for the palette in force (`{muted}`).
pub fn muted(p: &Palette) -> [u8; 3] {
    if lightness(p.window) < 128 {
        [0x8a, 0x94, 0xa0]
    } else {
        [0x5a, 0x64, 0x72]
    }
}

/// The palette for `mode` (default: the saved one) under the desktop's
/// scheme.
pub fn palette_for(mode: Option<&str>, system_dark: Option<bool>) -> Palette {
    let mode = mode.map_or_else(saved_theme, str::to_string);
    if is_dark(&mode, system_dark) {
        dark_palette()
    } else {
        light_palette()
    }
}
