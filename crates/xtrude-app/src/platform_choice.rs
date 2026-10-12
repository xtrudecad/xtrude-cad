// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Which windowing backend to start on (Preferences ▸ Display ▸ Graphics
//! server).
//!
//! Automatic leaves the default: Wayland when the session has it. Measured
//! on a laptop with Intel integrated graphics (Mesa, KDE Plasma 6, one 4K
//! monitor at 125 %), orbiting a model for 20 s: Wayland and X11
//! (XWayland) both drew at the monitor's 60 Hz (frame interval p50
//! 16.8 ms on both, p90 17.5 ms vs 17.9 ms), both sharp, with menus,
//! context menus, toolbar drag-and-drop and file dialogs working on both. So nothing forces X11 on KDE Plasma or at a fractional
//! scale; the user can still ask for X11 here, and an explicit
//! `XTRUDE_PLATFORM` in the environment always wins.
//!
//! `XTRUDE_FRAME_STATS=1` prints frame times on exit, to measure the two
//! backends against each other ([`crate::shell::frame_stats`]).

use std::collections::HashMap;

pub const AUTO: &str = "auto";
pub const WAYLAND: &str = "wayland";
pub const XCB: &str = "xcb";

/// The variable that overrides the choice.
pub const ENV_OVERRIDE: &str = "XTRUDE_PLATFORM";

/// The backend to force, or `None` to leave the default (Wayland when the
/// session has it).
pub fn choose_platform(preference: &str, env: &HashMap<String, String>) -> Option<&'static str> {
    let get = |k: &str| env.get(k).map(String::as_str).unwrap_or("");
    if !get(ENV_OVERRIDE).is_empty() {
        return None;
    }
    if preference.to_lowercase() == XCB && !get("DISPLAY").is_empty() {
        return Some(XCB);
    }
    None
}

/// Before the event loop exists: the backend the preference (and the
/// session) asks for. The caller hides `WAYLAND_DISPLAY` from the windowing
/// library when this is [`XCB`]. Returns the choice, or `None`.
pub fn platform_for(preference: &str) -> Option<&'static str> {
    let env: HashMap<String, String> = std::env::vars().collect();
    choose_platform(preference, &env)
}

/// Whether the app runs on Wayland: the forced `backend` says so, else
/// the session has a Wayland display. An empty `WAYLAND_DISPLAY` counts
/// as unset, as for every variable read here.
pub fn on_wayland(backend: Option<&str>, env: &HashMap<String, String>) -> bool {
    match backend {
        Some(XCB) => false,
        Some(WAYLAND) => true,
        _ => env.get("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
    }
}
