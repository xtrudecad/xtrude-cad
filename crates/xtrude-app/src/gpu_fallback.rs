// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Recover from a platform that cannot give us a GPU context.
//!
//! Some drivers (NVIDIA under GNOME Wayland) have no EGL config for a
//! context that the same driver serves under X11. Asking for less (no
//! stencil, no multisampling) is the wgpu adapter's business — the
//! viewport draws offscreen and picks its own sample count — so what is
//! left is the decision table here: restart under X11 at most once, and
//! say why when there is nowhere to go.

/// Marks the re-exec'd child, so a machine where X11 fails too gives up
/// with a diagnosis instead of restarting itself forever.
pub const GUARD_ENV: &str = "XTRUDE_GL_FALLBACK";

/// What became of the start (a word for the log and the About box).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The context we asked for works — the normal path.
    Ok,
    /// Restart under X11 (the caller re-execs).
    Restart,
    /// We are the restarted process and here the GPU works.
    Xcb,
    /// Wayland cannot, and there is no X11 to escape to.
    NoX11,
    /// No context anywhere; the caller reports it.
    Failed,
}

/// The environment the decision reads (the process's, in `main`).
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub guard: bool,
    pub display: bool,
}

impl Env {
    pub fn current() -> Env {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Env {
            guard: set(GUARD_ENV),
            display: set("DISPLAY"),
        }
    }
}

/// The window came up.
pub fn on_success(env: &Env) -> Outcome {
    if env.guard {
        Outcome::Xcb
    } else {
        Outcome::Ok
    }
}

/// No GPU context on `platform` (`"wayland"` or `"x11"`): what next.
pub fn on_failure(platform: &str, env: &Env) -> Outcome {
    if env.guard {
        return Outcome::Failed; // the X11 attempt is what just died
    }
    if !platform.starts_with("wayland") {
        return Outcome::Failed; // already on X11: nowhere left to go
    }
    if !env.display {
        // Wayland with no XWayland (or a Flatpak given only
        // --socket=fallback-x11): nothing to fall back to.
        return Outcome::NoX11;
    }
    Outcome::Restart
}

/// The message printed for an outcome, if any.
pub fn message(outcome: Outcome) -> Option<&'static str> {
    match outcome {
        Outcome::Restart => Some(
            "Xtrude CAD: this driver cannot create a GPU context under Wayland; restarting under X11.",
        ),
        Outcome::NoX11 => Some(
            "Xtrude CAD: this driver cannot create a GPU context under Wayland, and no X11 display is available to fall back on.",
        ),
        Outcome::Failed => Some(
            "Xtrude CAD: no GPU context on this machine — the viewport cannot draw.",
        ),
        Outcome::Ok | Outcome::Xcb => None,
    }
}
