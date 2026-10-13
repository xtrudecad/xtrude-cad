// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Covers the toolbars: the bars for parts, the toolbar areas'
//! mechanics, the factory layout, the saved arrangement (and one naming a
//! bar the parts layout replaced), moving, floating and hiding a bar (Window ▸
//! Toolbars), resetting the layout, and where a drop lands.

use crate::common;
use xtrude_model::settings::keys;

use std::collections::BTreeSet;

use serde_json::json;
use xtrude_app::window::toolbar_layout::{DockSpot, DropTarget, FloatingBar, LayoutGeometry, Rect};
use xtrude_app::window::{
    CtxItem, MainWindow, MenuItem, ToolbarArea, ToolbarLayout, PANEL_PREFIX, TOOLBAR_PANELS,
};

use ToolbarArea::{Bottom, Left, Right, Top};

fn known() -> Vec<String> {
    common::window()
        .toolbars
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect()
}

fn line(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Every known id exactly once, docked or floating, and no empty line.
fn assert_whole(layout: &ToolbarLayout, known: &[String]) {
    let mut all: Vec<String> = layout.lines.iter().flatten().flatten().cloned().collect();
    assert!(layout.lines.iter().flatten().all(|l| !l.is_empty()));
    all.extend(layout.floating.iter().map(|f| f.id.clone()));
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "a bar twice: {all:?}");
    let want: BTreeSet<&String> = known.iter().collect();
    assert_eq!(all.iter().collect::<BTreeSet<_>>(), want);
}

fn into(area: ToolbarArea, line: usize, index: usize) -> DropTarget {
    DropTarget::Into(DockSpot { area, line, index })
}

fn ids(win: &MainWindow, area: ToolbarArea) -> Vec<String> {
    win.toolbars_in(area).into_iter().map(|b| b.0).collect()
}

/// A layout in two areas with every bar shown: a top line, and Draw and
/// Annotate in a left column. The mechanics tests start from it.
fn two_areas() -> ToolbarLayout {
    ToolbarLayout {
        lines: [
            vec![line(&["main", "model", "view", "inspect", "transform"])],
            Vec::new(),
            vec![line(&["draw", "annotate"])],
            Vec::new(),
        ],
        floating: Vec::new(),
        hidden: BTreeSet::new(),
    }
}

/// A window laid out as [`two_areas`], saved as a user's layout would be.
fn two_areas_window() -> MainWindow {
    let mut win = common::window();
    win.toolbar_layout = two_areas();
    win.toolbar_layout.save();
    win
}

/// The factory layout as stored in the settings.
fn factory_json() -> serde_json::Value {
    json!({
        "top": [["main", "draw", "model", "transform", "inspect", "view"]],
        "bottom": [],
        "left": [["annotate"]],
        "right": [],
        "floating": [],
        "hidden": ["annotate"],
    })
}

#[test]
fn test_the_factory_layout_is_one_top_line_in_the_order_of_the_work() {
    let win = common::window();
    assert_eq!(
        win.toolbar_layout.lines[Top.index()],
        vec![line(&[
            "main",
            "draw",
            "model",
            "transform",
            "inspect",
            "view"
        ])]
    );
    assert_eq!(
        win.toolbar_layout.lines[Left.index()],
        vec![line(&["annotate"])]
    );
    assert!(win.toolbar_layout.lines[Bottom.index()].is_empty());
    assert!(win.toolbar_layout.lines[Right.index()].is_empty());
    assert!(win.toolbar_layout.floating.is_empty());
    assert_eq!(
        win.toolbar_layout.hidden,
        ["annotate"].map(String::from).into()
    );
    assert_eq!(win.toolbar_layout, ToolbarLayout::factory());
    assert_eq!(win.toolbar_layout.to_json(), factory_json());
    // what shows: one top line, a free left edge
    assert_eq!(
        ids(&win, Top),
        ["main", "draw", "model", "transform", "inspect", "view"]
    );
    assert!(ids(&win, Left).is_empty());
    assert_whole(&win.toolbar_layout, &known());
}

#[test]
fn test_save_and_load_round_trip_every_area_line_float_and_flag() {
    let known = known();
    let mut layout = ToolbarLayout::factory();
    layout.lines = [
        vec![line(&["main", "model"]), line(&["view"])],
        vec![line(&["inspect"])],
        vec![line(&["draw"])],
        vec![line(&["annotate"])],
    ];
    layout.floating = vec![FloatingBar {
        id: "transform".into(),
        pos: (412.5, 301.25),
    }];
    layout.hidden = ["inspect".to_string(), "transform".to_string()].into();
    layout.save();
    assert_eq!(ToolbarLayout::load(&known), layout);
    // and a new window starts from it
    let win = MainWindow::new();
    assert_eq!(win.toolbar_layout, layout);
    assert_eq!(ids(&win, Top), ["main", "model", "view"]);
    assert_eq!(win.floating_toolbars().len(), 0); // Transform is hidden
}

#[test]
fn test_a_corrupt_wrong_or_empty_value_gives_the_factory_layout() {
    let known = known();
    for bad in [
        json!("{\"top\": [[\"main\""),
        json!(5),
        json!(["main"]),
        json!({"top": "main"}),
        json!({"top": [["main", 3]]}),
        json!({"floating": [{"id": "transform"}]}),
        json!({"hidden": "transform"}),
        json!({}),
        json!({"top": [], "left": []}),
    ] {
        xtrude_model::settings::set(keys::TOOLBAR_LAYOUT.name(), bad.clone());
        assert_eq!(
            ToolbarLayout::load(&known),
            ToolbarLayout::factory(),
            "{bad}"
        );
    }
}

#[test]
fn test_unknown_ids_are_dropped_and_missing_ones_join_the_top_line() {
    let known = known();
    xtrude_model::settings::set(
        keys::TOOLBAR_LAYOUT.name(),
        json!({
            "top": [["main", "model"], ["view", "transform"]],
            "left": [["draw", "walkthrough", "annotate"], ["walkthrough"]],
            "floating": [{"id": "ghost", "pos": [1.0, 2.0]}],
            "hidden": ["ghost", "transform"],
        }),
    );
    let layout = ToolbarLayout::load(&known);
    assert_eq!(
        layout.lines[Left.index()],
        vec![line(&["draw", "annotate"])]
    );
    assert!(layout.floating.is_empty());
    assert_eq!(layout.hidden, ["transform".to_string()].into());
    // inspect was not saved: where a new bar goes, the end of the
    // innermost top line
    assert_eq!(
        layout.lines[Top.index()],
        vec![
            line(&["main", "model"]),
            line(&["view", "transform", "inspect"])
        ]
    );
    assert_whole(&layout, &known);
}

#[test]
fn test_a_bar_saved_twice_is_kept_once() {
    let known = known();
    xtrude_model::settings::set(
        keys::TOOLBAR_LAYOUT.name(),
        json!({"top": [["main", "main"]], "floating": [{"id": "main", "pos": [0, 0]}]}),
    );
    let layout = ToolbarLayout::load(&known);
    assert_whole(&layout, &known);
    assert_eq!(layout.lines[Top.index()][0][0], "main");
}

#[test]
fn test_place_within_a_line_forward_and_backward() {
    let known = known();
    let mut l = two_areas();
    // main dropped before inspect (index 3, counted with main still in
    // the line)
    l.place("main", into(Top, 0, 3));
    assert_eq!(
        l.lines[Top.index()][0],
        line(&["model", "view", "main", "inspect", "transform"])
    );
    // and back to the front
    l.place("main", into(Top, 0, 0));
    assert_eq!(l, two_areas());
    // past the last bar
    l.place("model", into(Top, 0, 5));
    assert_eq!(l.lines[Top.index()][0].last().unwrap(), "model");
    // onto its own place: nothing moves
    let before = l.clone();
    l.place("view", into(Top, 0, 1));
    l.place("view", into(Top, 0, 2));
    assert_eq!(l, before);
    assert_whole(&l, &known);
}

#[test]
fn test_place_to_other_lines_and_areas() {
    let known = known();
    let mut l = two_areas();
    // a new line at the inner end of the top area
    l.place("annotate", DropTarget::NewLine { area: Top, line: 1 });
    assert_eq!(l.lines[Top.index()][1], line(&["annotate"]));
    // into that second line
    l.place("view", into(Top, 1, 0));
    assert_eq!(l.lines[Top.index()][1], line(&["view", "annotate"]));
    // a new line at 0 (outermost)
    l.place("transform", DropTarget::NewLine { area: Top, line: 0 });
    assert_eq!(l.lines[Top.index()].len(), 3);
    assert_eq!(l.lines[Top.index()][0], line(&["transform"]));
    // to another area: the bottom and the right open their first line
    l.place(
        "inspect",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    l.place(
        "draw",
        DropTarget::NewLine {
            area: Right,
            line: 0,
        },
    );
    assert_eq!(l.lines[Bottom.index()], vec![line(&["inspect"])]);
    assert_eq!(l.lines[Right.index()], vec![line(&["draw"])]);
    assert!(l.lines[Left.index()].is_empty());
    // a line emptied by the move closes; into a line the area does not
    // have opens it
    l.place("transform", into(Left, 0, 1));
    assert_eq!(l.lines[Top.index()].len(), 2);
    assert_eq!(l.lines[Left.index()][0], line(&["transform"]));
    // emptying a line before the target shifts the target line down
    l.place(
        "inspect",
        DropTarget::NewLine {
            area: Bottom,
            line: 1,
        },
    );
    assert_eq!(l.lines[Bottom.index()], vec![line(&["inspect"])]);
    l.place("main", DropTarget::NewLine { area: Top, line: 2 });
    let top = &l.lines[Top.index()];
    assert_eq!(top[0], line(&["model"]));
    assert_eq!(top[1], line(&["view", "annotate"]));
    assert_eq!(top[2], line(&["main"]));
    l.place("model", into(Top, 2, 1));
    let top = &l.lines[Top.index()];
    assert_eq!(top.len(), 2);
    assert_eq!(top[1], line(&["main", "model"]));
    // a bar alone on its line dropped into that same line stays put
    l.place("draw", into(Right, 0, 0));
    assert_eq!(l.lines[Right.index()], vec![line(&["draw"])]);
    assert_whole(&l, &known);
}

#[test]
fn test_float_and_dock_again_keeps_every_bar_once() {
    let known = known();
    let mut l = two_areas();
    l.place(
        "draw",
        DropTarget::Float {
            pos: (300.0, 200.0),
        },
    );
    assert!(l.is_floating("draw"));
    assert_eq!(l.spot("draw"), None);
    assert_eq!(l.lines[Left.index()], vec![line(&["annotate"])]);
    assert_whole(&l, &known);
    // dragged again while floating: it moves
    l.place("draw", DropTarget::Float { pos: (10.0, 20.0) });
    assert_eq!(l.floating.len(), 1);
    assert_eq!(l.floating[0].pos, (10.0, 20.0));
    // the last left bar floats: the left area empties
    l.place("annotate", DropTarget::Float { pos: (50.0, 60.0) });
    assert!(l.lines[Left.index()].is_empty());
    assert_whole(&l, &known);
    // docked again
    l.place(
        "draw",
        DropTarget::NewLine {
            area: Left,
            line: 0,
        },
    );
    l.place("annotate", into(Left, 0, 1));
    assert_eq!(l, two_areas());
}

#[test]
fn test_a_hidden_bar_stays_hidden_after_a_restart() {
    let mut win = two_areas_window();
    win.toggle_toolbar("annotate");
    assert_eq!(ids(&win, Left), ["draw"]);
    let win = MainWindow::new();
    assert!(!win.toolbar_layout.is_shown("annotate"));
    assert_eq!(ids(&win, Left), ["draw"]);
    // still in its place, for when it comes back
    assert_eq!(win.toolbar_area("annotate"), Some(Left));
    let mut win = win;
    win.toggle_toolbar("annotate");
    assert_eq!(ids(&win, Left), ["draw", "annotate"]);
}

#[test]
fn test_every_move_is_saved_at_once() {
    let mut win = common::window();
    win.toolbar_layout = two_areas(); // every bar shown, nothing saved yet
    assert!(xtrude_model::settings::get(keys::TOOLBAR_LAYOUT.name()).is_none());
    win.place_toolbar(
        "transform",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    assert_eq!(MainWindow::new().toolbar_area("transform"), Some(Bottom));
    win.place_toolbar("transform", DropTarget::Float { pos: (5.0, 6.0) });
    let win = MainWindow::new();
    assert_eq!(win.toolbar_area("transform"), None);
    assert_eq!(win.floating_toolbars()[0].0 .0, "transform");
    assert_eq!(win.floating_toolbars()[0].1, (5.0, 6.0));
}

/// Window ▸ Toolbars as `(id, title, ticked)`; `None` for a separator.
fn toolbars_menu(win: &MainWindow) -> Vec<Option<(String, String, Option<bool>)>> {
    win.toolbars_menu()
        .into_iter()
        .map(|i| match i {
            CtxItem::Entry {
                id, text, checked, ..
            } => Some((id, text, checked)),
            _ => None,
        })
        .collect()
}

#[test]
fn test_the_toolbars_menu_lists_the_toolbars_then_the_reset() {
    // the toolbars in the order they were made, a separator, the reset
    let mut win = two_areas_window();
    let menu = toolbars_menu(&win);
    let titles: Vec<String> = menu
        .iter()
        .map(|e| e.as_ref().map(|e| e.1.clone()).unwrap_or_default())
        .collect();
    assert_eq!(
        titles,
        [
            "Main",
            "Draw",
            "Model",
            "Transform",
            "Inspect",
            "View",
            "Annotate",
            "",
            "Reset toolbar layout"
        ]
    );
    assert!(menu[..7].iter().flatten().all(|e| e.2 == Some(true)));
    let reset = menu.last().unwrap().as_ref().unwrap();
    assert_eq!((reset.0.as_str(), reset.2), ("reset_toolbar_layout", None));
    win.trigger("toolbar:transform");
    assert!(!win.toolbar_layout.is_shown("transform"));
    let transform = toolbars_menu(&win)
        .into_iter()
        .flatten()
        .find(|e| e.0 == "toolbar:transform")
        .unwrap();
    assert_eq!(transform.2, Some(false));
    win.trigger("toolbar:transform");
    assert!(win.toolbar_layout.is_shown("transform"));
}

#[test]
fn test_the_toolbars_menu_is_in_the_window_menu() {
    let win = common::window();
    let (_, window_menu) = win.menus.iter().find(|(t, _)| t == "Window").unwrap();
    assert!(window_menu.iter().any(|i| matches!(
        i,
        MenuItem::Menu(t, sub) if t == "Toolbars"
            && matches!(sub.as_slice(), [MenuItem::Dynamic("toolbars")])
    )));
}

#[test]
fn test_reset_toolbar_layout_restores_the_factory_layout_and_saves_it() {
    // before the window: a new window starts from fresh settings
    let known = known();
    let mut win = common::window();
    win.place_toolbar(
        "transform",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    win.place_toolbar("draw", DropTarget::Float { pos: (50.0, 60.0) });
    win.place_toolbar("main", into(Right, 0, 0));
    win.toggle_toolbar("annotate");
    win.toggle_toolbar("transform");
    assert_ne!(win.toolbar_layout, ToolbarLayout::factory());

    assert!(win.trigger("reset_toolbar_layout"));
    assert_eq!(win.toolbar_layout, ToolbarLayout::factory());
    let shown: Vec<&String> = known
        .iter()
        .filter(|id| win.toolbar_layout.is_shown(id))
        .collect();
    let factory = ["main", "draw", "model", "transform", "inspect", "view"];
    assert_eq!(shown, factory);
    assert!(win.floating_toolbars().is_empty());
    assert_eq!(ids(&win, Top), factory);
    assert!(ids(&win, Left).is_empty());
    // saved at once, as the factory layout: the setting reloads to it
    assert_eq!(
        xtrude_model::settings::get(keys::TOOLBAR_LAYOUT.name()),
        Some(factory_json())
    );
    assert_eq!(ToolbarLayout::load(&known), ToolbarLayout::factory());
    assert_eq!(MainWindow::new().toolbar_layout, ToolbarLayout::factory());
    // a toolbar change, not a document change: nothing to undo
    assert!(!win.viewport.history.can_undo());
}

#[test]
fn test_a_saved_layout_loads_as_it_is() {
    // no migration to the factory layout: Draw stays on the left, every
    // bar stays shown
    let known = known();
    two_areas().save();
    assert_eq!(ToolbarLayout::load(&known), two_areas());
    let win = MainWindow::new();
    assert_eq!(win.toolbar_layout, two_areas());
    assert_eq!(ids(&win, Left), ["draw", "annotate"]);
}

#[test]
fn test_showing_annotate_from_the_toolbars_menu_docks_it_on_the_left() {
    let mut win = common::window();
    assert!(!win.toolbar_layout.is_shown("annotate"));
    let entry = toolbars_menu(&win)
        .into_iter()
        .flatten()
        .find(|e| e.0 == "toolbar:annotate")
        .unwrap();
    assert_eq!(entry.2, Some(false));
    win.trigger("toolbar:annotate");
    assert!(win.toolbar_layout.is_shown("annotate"));
    assert_eq!(win.toolbar_area("annotate"), Some(Left));
    assert_eq!(ids(&win, Left), ["annotate"]);
    // and it stays there after a restart
    assert_eq!(ids(&MainWindow::new(), Left), ["annotate"]);
}

#[test]
fn test_reset_toolbar_layout_has_no_shortcut_and_names_what_it_leaves() {
    let win = common::window();
    let a = win.actions.get("reset_toolbar_layout").unwrap();
    assert_eq!(a.text, "Reset toolbar layout");
    assert!(a.shortcuts.is_empty() && !a.checkable);
    assert!(a.status_tip.contains("side panels stay"));
}

#[test]
fn test_reset_toolbar_layout_leaves_hidden_trays_hidden() {
    let mut win = common::window();
    win.set_tray_shown("tray_properties", false);
    win.toggle_toolbar("main");
    win.trigger("reset_toolbar_layout");
    assert!(win.toolbar_layout.is_shown("main"));
    assert!(!win.tray_shown("tray_properties"));
    assert!(!MainWindow::new().tray_shown("tray_properties"));
}

#[test]
fn test_clean_screen_leaves_the_saved_layout_untouched() {
    let mut win = common::window();
    win.place_toolbar(
        "transform",
        DropTarget::NewLine {
            area: Right,
            line: 0,
        },
    );
    win.toggle_toolbar("main");
    let saved =
        serde_json::to_string(&xtrude_model::settings::get(keys::TOOLBAR_LAYOUT.name())).unwrap();
    win.trigger("clean_screen");
    win.trigger("clean_screen");
    let after =
        serde_json::to_string(&xtrude_model::settings::get(keys::TOOLBAR_LAYOUT.name())).unwrap();
    assert_eq!(saved, after);
}

/// A 1000 × 700 window: a top area with one 40-pt line holding main
/// (0..200) and model (200..400) from y 30; a left column 40 pt wide
/// holding draw from y 70; empty bottom (above a status bar at 680) and
/// right areas.
fn geometry() -> (ToolbarLayout, LayoutGeometry) {
    let mut l = two_areas();
    l.lines[Top.index()] = vec![line(&["main", "model"])];
    l.lines[Left.index()] = vec![line(&["draw"])];
    let g = LayoutGeometry {
        areas: vec![
            (Top, Rect::new(0.0, 30.0, 1000.0, 70.0)),
            (Bottom, Rect::new(0.0, 680.0, 1000.0, 680.0)),
            (Left, Rect::new(0.0, 70.0, 40.0, 680.0)),
            (Right, Rect::new(1000.0, 70.0, 1000.0, 680.0)),
        ],
        lines: vec![
            (Top, 0, Rect::new(0.0, 30.0, 1000.0, 70.0)),
            (Left, 0, Rect::new(0.0, 70.0, 40.0, 680.0)),
        ],
        bars: vec![
            ("main".into(), Rect::new(0.0, 30.0, 200.0, 70.0)),
            ("model".into(), Rect::new(200.0, 30.0, 400.0, 70.0)),
            ("draw".into(), Rect::new(0.0, 70.0, 40.0, 300.0)),
        ],
    };
    (l, g)
}

#[test]
fn test_drop_target_on_a_bar_goes_before_or_after_by_its_half() {
    let (l, g) = geometry();
    let grab = (4.0, 4.0);
    assert_eq!(l.drop_target(&g, (60.0, 50.0), grab), into(Top, 0, 0));
    assert_eq!(l.drop_target(&g, (150.0, 50.0), grab), into(Top, 0, 1));
    assert_eq!(l.drop_target(&g, (250.0, 50.0), grab), into(Top, 0, 1));
    assert_eq!(l.drop_target(&g, (350.0, 50.0), grab), into(Top, 0, 2));
    // vertical: by the half along the column
    assert_eq!(l.drop_target(&g, (20.0, 100.0), grab), into(Left, 0, 0));
    assert_eq!(l.drop_target(&g, (20.0, 250.0), grab), into(Left, 0, 1));
}

#[test]
fn test_drop_target_past_the_last_bar_is_the_end_of_the_line() {
    let (l, g) = geometry();
    assert_eq!(
        l.drop_target(&g, (800.0, 50.0), (0.0, 0.0)),
        into(Top, 0, 2)
    );
    assert_eq!(
        l.drop_target(&g, (20.0, 500.0), (0.0, 0.0)),
        into(Left, 0, 1)
    );
}

#[test]
fn test_drop_target_inside_an_inner_edge_opens_a_new_inner_line() {
    let (l, g) = geometry();
    let new = |area, line| DropTarget::NewLine { area, line };
    // below the top line, within 80 pt
    assert_eq!(l.drop_target(&g, (500.0, 120.0), (0.0, 0.0)), new(Top, 1));
    // right of the left column
    assert_eq!(l.drop_target(&g, (90.0, 400.0), (0.0, 0.0)), new(Left, 1));
    // the empty bottom and right areas, within 80 pt of their edge
    assert_eq!(
        l.drop_target(&g, (500.0, 640.0), (0.0, 0.0)),
        new(Bottom, 0)
    );
    assert_eq!(l.drop_target(&g, (950.0, 400.0), (0.0, 0.0)), new(Right, 0));
    // two areas in reach: the nearer wins, a tie goes to the first in
    // the order left, right, top, bottom (left)
    assert_eq!(l.drop_target(&g, (60.0, 85.0), (0.0, 0.0)), new(Top, 1));
    assert_eq!(l.drop_target(&g, (60.0, 90.0), (0.0, 0.0)), new(Left, 1));
    assert_eq!(l.drop_target(&g, (45.0, 140.0), (0.0, 0.0)), new(Left, 1));
}

#[test]
fn test_drop_target_beyond_the_outermost_line_or_in_the_open_floats() {
    let (l, g) = geometry();
    let grab = (5.0, 8.0);
    // over the menu bar, above the top line
    assert_eq!(
        l.drop_target(&g, (300.0, 10.0), grab),
        DropTarget::Float { pos: (295.0, 2.0) }
    );
    // the open centre
    assert_eq!(
        l.drop_target(&g, (500.0, 400.0), grab),
        DropTarget::Float {
            pos: (495.0, 392.0)
        }
    );
}

#[test]
fn test_a_layout_naming_a_replaced_bar_loads_as_the_factory_layout_once() {
    let known = known();
    // a bar the parts layout replaced, docked, floating or only hidden
    for saved in [
        json!({"top": [["main", "modify", "view"]], "left": [["draw"]]}),
        json!({"top": [["main", "draw"]], "bottom": [["sections"]]}),
        json!({"top": [["main"]], "floating": [{"id": "solids", "pos": [1.0, 2.0]}]}),
        json!({"top": [["main", "draw", "view"]], "hidden": ["panels"]}),
    ] {
        xtrude_model::settings::set(keys::TOOLBAR_LAYOUT.name(), saved.clone());
        assert_eq!(
            ToolbarLayout::load(&known),
            ToolbarLayout::factory(),
            "{saved}"
        );
        // saved at once, so it happens once
        assert_eq!(
            xtrude_model::settings::get(keys::TOOLBAR_LAYOUT.name()),
            Some(factory_json()),
            "{saved}"
        );
    }
    // from then on the user's moves stay
    let mut win = MainWindow::new();
    assert_eq!(win.toolbar_layout, ToolbarLayout::factory());
    win.place_toolbar("inspect", into(Left, 0, 0));
    assert_eq!(MainWindow::new().toolbar_area("inspect"), Some(Left));
}

#[test]
fn test_the_bars_hold_the_parts_tools_in_the_order_of_the_work() {
    let win = common::window();
    let bars: Vec<(&str, &str, Vec<&str>)> = win
        .toolbars
        .iter()
        .map(|(id, title, ids)| {
            (
                id.as_str(),
                title.as_str(),
                ids.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        bars,
        [
            (
                "main",
                "Main",
                vec!["save_tb", "undo", "redo", "select", "eraser", "stack:paint"]
            ),
            (
                "draw",
                "Draw",
                vec![
                    "stack:lines",
                    "stack:rectangles",
                    "stack:circles",
                    "stack:arcs"
                ]
            ),
            (
                "model",
                "Model",
                vec![
                    "pushpull",
                    "hole",
                    "sweep",
                    "offset",
                    "stack:edges",
                    "shell",
                    "stack:booleans",
                    "solid_split",
                    "merge_solids",
                    "text3d"
                ]
            ),
            (
                "transform",
                "Transform",
                vec!["move", "rotate", "scale", "flip", "stack:patterns"]
            ),
            (
                "inspect",
                "Inspect",
                vec![
                    "measure",
                    "protractor",
                    "section",
                    "solid_inspector",
                    "model_info"
                ]
            ),
            (
                "view",
                "View",
                vec![
                    "stack:nav",
                    "zoom_extents",
                    "zoom_selection",
                    "stack:views",
                    "toggle_projection",
                    "panel:styles"
                ]
            ),
            (
                "annotate",
                "Annotate",
                vec!["dimension", "text", "panel:dimension_style"]
            ),
        ]
    );
    // every plain entry is an action with an icon of its own; every panel
    // entry is a known drop-down
    for (_, _, ids) in &bars {
        for id in ids {
            if let Some(panel) = id.strip_prefix(PANEL_PREFIX) {
                assert!(TOOLBAR_PANELS.iter().any(|(k, _)| *k == panel), "{id}");
            } else if !id.starts_with("stack:") {
                let a = win.actions.get(id).unwrap_or_else(|| panic!("{id}"));
                assert!(a.icon.is_some(), "{id} has no icon");
            }
        }
    }
    // no bar under four buttons but Annotate, which is hidden
    for (id, _, ids) in &bars {
        assert!(ids.len() >= 4 || *id == "annotate", "{id}");
    }
}
