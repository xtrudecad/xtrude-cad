// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Xtrude CAD application: window, tools, panels, preferences.

/// The application id, `org.xtrudecad.XtrudeCAD`: the id the window
/// reports (Wayland `app_id`, X11 `WM_CLASS`), and the name of the desktop
/// file, the AppStream metainfo, the icons, the MIME package and the
/// Flatpak. Desktops match the window to its desktop file by it. It is
/// permanent once published; settings, crash logs and extensions keep
/// their `xtrude` folders.
pub const APP_ID: &str = "org.xtrudecad.XtrudeCAD";

pub mod fontmetrics;
pub mod gpu_fallback;
pub mod input;
pub mod platform_choice;
pub mod shell;
pub mod theme;
pub mod tools;
pub mod tray;
pub mod value_box;
pub mod viewport;
pub mod window;
