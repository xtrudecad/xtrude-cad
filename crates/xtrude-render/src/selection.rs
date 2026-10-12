// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The selection cue's colours.

/// The selection dots' orange.
pub const SELECTION_DOT_COLOR: [f64; 3] = [0.95, 0.45, 0.16];
/// The dots on a surface that is orange or red itself.
pub const SELECTION_DOT_ALT_COLOR: [f64; 3] = [0.10, 0.25, 0.85];

/// RGB → hue, saturation, value, all in 0–1.
fn rgb_to_hsv([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let v = maxc;
    if minc == maxc {
        return (0.0, 0.0, v);
    }
    let s = (maxc - minc) / maxc;
    let rc = (maxc - r) / (maxc - minc);
    let gc = (maxc - g) / (maxc - minc);
    let bc = (maxc - b) / (maxc - minc);
    let h = if r == maxc {
        bc - gc
    } else if g == maxc {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    ((h / 6.0).rem_euclid(1.0), s, v)
}

/// The dot colour that reads on a face side painted `surface` (`None` when
/// unknown): the selection orange, unless the surface is orange-ish or
/// red-ish itself (hue within ~50° of red, saturated and bright enough to
/// look coloured), where the dots turn blue.
pub fn selection_dot_color(surface: Option<[f64; 3]>) -> [f64; 3] {
    let Some(c) = surface else {
        return SELECTION_DOT_COLOR;
    };
    let (h, s, v) = rgb_to_hsv(c);
    if s >= 0.35 && v >= 0.35 && (h <= 0.14 || h >= 0.96) {
        return SELECTION_DOT_ALT_COLOR;
    }
    SELECTION_DOT_COLOR
}
