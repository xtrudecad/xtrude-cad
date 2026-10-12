// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The status bar shows ONE hint for the tool and its step, not a strip of
//! every shortcut at once. The bar is the toolkit-neutral `StatusBar`.

use crate::common;

use glam::DVec3;
use xtrude_app::tools::arc::ArcTool;
use xtrude_app::tools::fillet::FilletTool;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::{NavMode, NavStyle};
use xtrude_app::window::hints::{has_hint, hint_for, nav_hint};
use xtrude_app::window::status_bar::StatusBar;
use xtrude_app::window::tool_keys;

#[test]
fn test_the_hint_follows_the_tools_step() {
    let mut line = LineTool::default();
    assert!(hint_for(Some("line"), Some(&line), None, None).starts_with("Click the start point"));
    line.start_point = Some(DVec3::ZERO);
    assert!(hint_for(Some("line"), Some(&line), None, None).starts_with("Click the end point"));
    let mut arc = ArcTool::new();
    assert_eq!(arc.phase(), "idle");
    arc.start_point = Some(DVec3::ZERO);
    assert_eq!(arc.phase(), "end");
    arc.end_point = Some(DVec3::X);
    assert_eq!(arc.phase(), "bulge");
    assert!(hint_for(Some("arc"), Some(&arc), None, None).contains("Ns"));
    let mut fil = FilletTool::default();
    assert!(hint_for(Some("fillet"), Some(&fil), None, None).contains("radius"));
    fil.sizing = true;
    assert_eq!(fil.phase(), "sizing");
}

#[test]
fn test_navigation_and_unknown_tools_have_a_line_too() {
    // A tool with no hint of its own leads with its name.
    let line = LineTool::default();
    assert!(hint_for(None, Some(&line), None, None).starts_with(line.info().name));
    assert_eq!(
        hint_for(
            Some("line"),
            Some(&line),
            Some((NavMode::Orbit, NavStyle::Default)),
            None
        ),
        nav_hint(NavMode::Orbit, NavStyle::Default)
    );
}

#[test]
fn test_every_registered_tool_has_a_hint() {
    let mut win = common::window();
    let missing: Vec<_> = tool_keys().into_iter().filter(|k| !has_hint(k)).collect();
    assert!(missing.is_empty(), "{missing:?}");
    win.activate_tool("line");
    // The tool's name leads its hint.
    assert!(win
        .status_hint()
        .starts_with("Line — Click the start point"));
    let mut bar = StatusBar::default();
    bar.set_standing(&win.status_hint());
    assert!(bar
        .current_message()
        .starts_with("Line — Click the start point"));
    win.activate_nav("orbit");
    bar.set_standing(&win.status_hint());
    assert!(win.status_hint().starts_with("Orbit — Drag to orbit"));
    // A timed message covers the hint and the hint comes back.
    bar.show_message("Guide at 2.2 m", 50, 0.0);
    assert!(bar.current_message().starts_with("Guide at"));
    bar.restore();
    assert!(bar.current_message().starts_with("Orbit — Drag to orbit"));
}

#[test]
fn test_the_message_keeps_to_its_share_of_the_bar() {
    assert_eq!(
        StatusBar::message_max_width(1600.0),
        (1600.0 * 0.6f64).trunc()
    );
    assert_eq!(StatusBar::message_max_width(200.0), 120.0); // never squeezed to nothing
}
