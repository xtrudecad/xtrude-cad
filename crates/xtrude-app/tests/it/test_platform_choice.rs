// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Automatic leaves the session's default (Wayland when it has it), on KDE
//! Plasma and at a fractional scale too; only the X11 preference forces
//! X11, and an explicit `XTRUDE_PLATFORM` in the environment always wins.

use std::collections::HashMap;
use xtrude_app::platform_choice::{choose_platform, on_wayland, AUTO, WAYLAND, XCB};

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn wayland_with_xwayland() -> HashMap<String, String> {
    env(&[("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":0")])
}

#[test]
fn test_auto_never_forces_x11() {
    let wl = wayland_with_xwayland();
    assert_eq!(choose_platform(AUTO, &wl), None);
    assert_eq!(choose_platform("", &wl), None);
    for desktop in ["KDE", "ubuntu:GNOME"] {
        let mut e = wl.clone();
        e.insert("XDG_CURRENT_DESKTOP".into(), desktop.into());
        assert_eq!(choose_platform(AUTO, &e), None, "{desktop}");
    }
    assert_eq!(
        choose_platform(
            AUTO,
            &env(&[("XDG_SESSION_TYPE", "x11"), ("DISPLAY", ":0")])
        ),
        None
    );
}

#[test]
fn test_the_explicit_choices() {
    let wl = wayland_with_xwayland();
    assert_eq!(choose_platform(XCB, &wl), Some(XCB));
    assert_eq!(choose_platform("XCB", &wl), Some(XCB));
    assert_eq!(
        choose_platform(XCB, &env(&[("XDG_SESSION_TYPE", "wayland")])),
        None
    );
    assert_eq!(choose_platform(WAYLAND, &wl), None);
}

#[test]
fn test_the_environment_override_wins() {
    let mut forced = wayland_with_xwayland();
    forced.insert("XTRUDE_PLATFORM".into(), "wayland".into());
    assert_eq!(choose_platform(XCB, &forced), None);
    assert_eq!(choose_platform(AUTO, &forced), None);
}

#[test]
fn test_an_empty_wayland_display_is_no_wayland_session() {
    let x11 = env(&[("DISPLAY", ":77"), ("WAYLAND_DISPLAY", "")]);
    assert!(!on_wayland(None, &x11), "an empty WAYLAND_DISPLAY is unset");
    assert!(!on_wayland(None, &env(&[("DISPLAY", ":0")])));
    let wl = env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
    assert!(on_wayland(None, &wl));
    // a forced backend says which, whatever the session has
    assert!(!on_wayland(Some(XCB), &wl));
    assert!(on_wayland(Some(WAYLAND), &x11));
}
