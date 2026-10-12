// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Stacked toolbar buttons: a bar holds `stack:<id>` in place of the
//! stack's members; the button shows the member run last (by any route),
//! or the first, and remembers it across restarts.

use crate::common;

use std::collections::HashSet;

use serde_json::json;
use xtrude_app::input::KeyEvent;
use xtrude_app::window::toolbar_stacks::{self, STACK_PREFIX, TOOLBAR_STACKS};
use xtrude_app::window::{MainWindow, Repeat, TOOLBAR_LAYOUT};
use xtrude_model::settings::{self, keys};

fn bar<'a>(w: &'a MainWindow, id: &str) -> &'a [String] {
    &w.toolbars.iter().find(|(b, _, _)| b == id).unwrap().2
}

/// A member of `stack` past its first that has a shortcut, pressed.
fn press_a_shortcut(w: &mut MainWindow, stack: &str) -> &'static str {
    let members = toolbar_stacks::members(stack).unwrap();
    let m = members[1..]
        .iter()
        .copied()
        .find(|m| !w.actions.get(m).unwrap().shortcuts.is_empty())
        .expect("a member with a shortcut");
    let seq = w.actions.get(m).unwrap().shortcuts[0];
    w.key_press(&KeyEvent::new(seq.key, "", seq.mods));
    m
}

#[test]
fn test_every_member_is_an_action_in_one_stack_and_on_no_bar() {
    let w = common::window();
    let mut seen = HashSet::new();
    for (id, _, members) in TOOLBAR_STACKS {
        assert!(members.len() > 1, "{id}");
        for m in members {
            assert!(w.actions.get(m).is_some(), "{m} is no action");
            assert!(seen.insert(*m), "{m} is in two stacks");
            assert!(
                w.toolbars
                    .iter()
                    .all(|(_, _, ids)| !ids.iter().any(|i| i == m)),
                "{m} is still a button of its own"
            );
        }
        let entry = format!("{STACK_PREFIX}{id}");
        let bars = w
            .toolbars
            .iter()
            .filter(|(_, _, ids)| ids.contains(&entry))
            .count();
        assert_eq!(bars, 1, "{entry} is on one bar");
    }
}

#[test]
fn test_every_tool_is_still_on_a_bar_alone_or_in_a_stack() {
    let w = common::window();
    let on_bars: HashSet<&str> = w
        .toolbars
        .iter()
        .flat_map(|(_, _, ids)| ids.iter())
        .flat_map(|i| match i.strip_prefix(STACK_PREFIX) {
            Some(s) => toolbar_stacks::members(s).unwrap().to_vec(),
            None => vec![i.as_str()],
        })
        .collect();
    for (_, _, tools) in TOOLBAR_LAYOUT {
        for t in tools {
            assert!(on_bars.contains(t), "{t} is on no bar");
        }
    }
    for v in ["iso", "top", "front", "right", "left", "back", "bottom"] {
        assert!(on_bars.contains(format!("view:{v}").as_str()), "{v}");
    }
}

#[test]
fn test_the_stacks_sit_where_their_members_were() {
    let w = common::window();
    // the views at the end of the View bar
    assert_eq!(bar(&w, "view").last().unwrap(), "stack:views");
    assert!(w.toolbars.iter().all(|(b, _, _)| b != "views"));
    let draw = bar(&w, "draw");
    assert_eq!(
        draw,
        [
            "line",
            "freehand",
            "stack:rectangles",
            "circle",
            "polygon",
            "stack:arcs",
        ]
    );
    let solids = bar(&w, "solids");
    assert_eq!(solids, ["merge_solids", "stack:booleans", "solid_split"]);
}

#[test]
fn test_a_stack_shows_its_first_member_by_default() {
    let w = common::window();
    for (id, _, members) in TOOLBAR_STACKS {
        assert_eq!(w.stack_current(id), Some(members[0]));
    }
    assert_eq!(w.stack_current("ghost"), None);
}

#[test]
fn test_picking_a_tool_runs_it_and_the_stack_shows_it() {
    let mut w = common::window();
    assert!(w.pick_in_stack("arcs", "center_arc"));
    assert_eq!(w.active_tool_key(), Some("center_arc"));
    assert_eq!(w.stack_current("arcs"), Some("center_arc"));
    // one Repeat entry, as its own button would leave
    assert!(matches!(w.last_command(), Some((_, Repeat::Tool(k))) if k == "center_arc"));
    // the other stacks did not move
    assert_eq!(w.stack_current("rectangles"), Some("rectangle"));
}

#[test]
fn test_picking_a_view_moves_the_camera() {
    let mut w = common::window();
    assert!(w.pick_in_stack("views", "view:top"));
    let f = w.viewport.camera.forward();
    assert!(f.z < -0.999, "looks down: {f:?}");
    assert_eq!(w.stack_current("views"), Some("view:top"));
}

#[test]
fn test_a_pick_outside_the_stack_does_nothing() {
    let mut w = common::window();
    assert!(!w.pick_in_stack("arcs", "line"));
    assert!(!w.pick_in_stack("ghost", "arc"));
    assert_eq!(w.active_tool_key(), Some("select"));
    assert_eq!(w.stack_current("arcs"), Some("arc"));
}

#[test]
fn test_the_stack_follows_a_member_run_by_its_shortcut() {
    let mut w = common::window();
    let m = press_a_shortcut(&mut w, "arcs");
    assert_eq!(w.active_tool_key(), Some(m));
    assert_eq!(w.stack_current("arcs"), Some(m));
    // and from the menu (an action triggered by id)
    w.trigger("rotated_rect");
    assert_eq!(w.stack_current("rectangles"), Some("rotated_rect"));
    w.trigger("view:front");
    assert_eq!(w.stack_current("views"), Some("view:front"));
}

#[test]
fn test_a_refused_tool_leaves_the_stack_as_it_was() {
    let mut w = common::window();
    w.tool_filter = Some(["select".to_string()].into());
    w.trigger("arc3");
    assert_eq!(w.stack_current("arcs"), Some("arc"));
}

#[test]
fn test_the_current_members_survive_a_restart() {
    let mut w = common::window();
    w.pick_in_stack("booleans", "solid_trim");
    w.trigger("view:left");
    let again = MainWindow::new();
    assert_eq!(again.stack_current("booleans"), Some("solid_trim"));
    assert_eq!(again.stack_current("views"), Some("view:left"));
    assert_eq!(again.stack_current("arcs"), Some("arc"));
}

#[test]
fn test_unknown_entries_in_the_setting_are_ignored() {
    let _w = common::window();
    settings::set(
        keys::TOOLBAR_STACKS.name(),
        json!({
            "ghost": "arc",
            "arcs": "line",
            "rectangles": 3,
            "views": "view:back",
        }),
    );
    let w = MainWindow::new();
    assert_eq!(w.stack_current("arcs"), Some("arc"));
    assert_eq!(w.stack_current("rectangles"), Some("rectangle"));
    assert_eq!(w.stack_current("views"), Some("view:back"));
    settings::set(keys::TOOLBAR_STACKS.name(), json!("not an object"));
    assert_eq!(MainWindow::new().stack_current("views"), Some("view:iso"));
}

#[test]
fn test_a_layout_saved_with_the_old_standard_views_bar_drops_it() {
    let _w = common::window();
    settings::set(
        keys::TOOLBAR_LAYOUT.name(),
        json!({
            "top": [["main", "views", "view"]],
            "floating": [{"id": "views", "pos": [1.0, 2.0]}],
            "hidden": ["views"],
        }),
    );
    let w = MainWindow::new();
    let all: Vec<&String> = w.toolbar_layout.lines.iter().flatten().flatten().collect();
    assert!(!all.iter().any(|b| *b == "views"));
    assert!(w.toolbar_layout.floating.is_empty());
    assert!(!w.toolbar_layout.hidden.contains("views"));
    // the views are on the View bar, which stays where it was
    assert_eq!(w.toolbar_layout.lines[0][0][..2], ["main", "view"]);
    assert_eq!(bar(&w, "view").last().unwrap(), "stack:views");
}
