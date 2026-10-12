// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Covers the toolbar areas' mechanics: the factory layout, the
//! saved arrangement, moving, floating and hiding a bar (Window ▸
//! Toolbars), resetting the layout, and where a drop lands.

use crate::common;
use xtrude_model::settings::keys;

use std::collections::BTreeSet;

use serde_json::json;
use xtrude_app::window::toolbar_layout::{DockSpot, DropTarget, FloatingBar, LayoutGeometry, Rect};
use xtrude_app::window::{CtxItem, MainWindow, MenuItem, ToolbarArea, ToolbarLayout};

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
            vec![line(&[
                "main", "modify", "view", "sections", "views", "solids", "panels",
            ])],
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
        "top": [["main", "modify", "view", "sections", "views", "solids", "panels", "draw"]],
        "bottom": [],
        "left": [["annotate"]],
        "right": [],
        "floating": [],
        "hidden": ["annotate", "panels", "solids", "views"],
    })
}

#[test]
fn test_the_factory_layout_is_one_top_row_with_draw_last() {
    let win = common::window();
    assert_eq!(
        win.toolbar_layout.lines[Top.index()],
        vec![line(&[
            "main", "modify", "view", "sections", "views", "solids", "panels", "draw"
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
        ["annotate", "panels", "solids", "views"]
            .map(String::from)
            .into()
    );
    assert_eq!(win.toolbar_layout, ToolbarLayout::factory());
    assert_eq!(win.toolbar_layout.to_json(), factory_json());
    // what shows: one top row, a free left edge
    assert_eq!(
        ids(&win, Top),
        ["main", "modify", "view", "sections", "draw"]
    );
    assert!(ids(&win, Left).is_empty());
    assert_whole(&win.toolbar_layout, &known());
    // the Panels bar is a toolbar like the others, with no actions
    let panels = win.toolbars.iter().find(|t| t.0 == "panels").unwrap();
    assert_eq!(panels.1, "Panels");
    assert!(panels.2.is_empty());
}

#[test]
fn test_save_and_load_round_trip_every_area_line_float_and_flag() {
    let known = known();
    let mut layout = ToolbarLayout::factory();
    layout.lines = [
        vec![line(&["main", "modify"]), line(&["view", "sections"])],
        vec![line(&["views"])],
        vec![line(&["draw"]), line(&["annotate"])],
        vec![line(&["solids"])],
    ];
    layout.floating = vec![FloatingBar {
        id: "panels".into(),
        pos: (412.5, 301.25),
    }];
    layout.hidden = ["sections".to_string(), "panels".to_string()].into();
    layout.save();
    assert_eq!(ToolbarLayout::load(&known), layout);
    // and a new window starts from it
    let win = MainWindow::new();
    assert_eq!(win.toolbar_layout, layout);
    assert_eq!(ids(&win, Top), ["main", "modify", "view"]);
    assert_eq!(win.floating_toolbars().len(), 0); // Panels is hidden
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
        json!({"floating": [{"id": "views"}]}),
        json!({"hidden": "views"}),
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
            "top": [["main", "modify"], ["view", "views", "sections"]],
            "left": [["draw", "walkthrough", "annotate"], ["walkthrough"]],
            "floating": [{"id": "ghost", "pos": [1.0, 2.0]}],
            "hidden": ["ghost", "views"],
        }),
    );
    let layout = ToolbarLayout::load(&known);
    assert_eq!(
        layout.lines[Left.index()],
        vec![line(&["draw", "annotate"])]
    );
    assert!(layout.floating.is_empty());
    assert_eq!(layout.hidden, ["views".to_string()].into());
    // solids and panels were not saved: where a new bar goes, the
    // end of the innermost top line
    assert_eq!(
        layout.lines[Top.index()],
        vec![
            line(&["main", "modify"]),
            line(&["view", "views", "sections", "solids", "panels"])
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
    // main dropped before sections (index 3, counted with main still in
    // the line)
    l.place("main", into(Top, 0, 3));
    assert_eq!(
        l.lines[Top.index()][0],
        line(&["modify", "view", "main", "sections", "views", "solids", "panels"])
    );
    // and back to the front
    l.place("main", into(Top, 0, 0));
    assert_eq!(l, two_areas());
    // past the last bar
    l.place("modify", into(Top, 0, 7));
    assert_eq!(l.lines[Top.index()][0].last().unwrap(), "modify");
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
    l.place("views", DropTarget::NewLine { area: Top, line: 1 });
    assert_eq!(l.lines[Top.index()][1], line(&["views"]));
    // into that second line
    l.place("view", into(Top, 1, 0));
    assert_eq!(l.lines[Top.index()][1], line(&["view", "views"]));
    // a new line at 0 (outermost)
    l.place("solids", DropTarget::NewLine { area: Top, line: 0 });
    assert_eq!(l.lines[Top.index()].len(), 3);
    assert_eq!(l.lines[Top.index()][0], line(&["solids"]));
    // to another area: the bottom and the right open their first line
    l.place(
        "sections",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    l.place(
        "panels",
        DropTarget::NewLine {
            area: Right,
            line: 0,
        },
    );
    assert_eq!(l.lines[Bottom.index()], vec![line(&["sections"])]);
    assert_eq!(l.lines[Right.index()], vec![line(&["panels"])]);
    // a line emptied by the move closes
    l.place("solids", into(Left, 0, 1));
    assert_eq!(l.lines[Top.index()].len(), 2);
    assert_eq!(
        l.lines[Left.index()][0],
        line(&["draw", "solids", "annotate"])
    );
    // emptying a line before the target shifts the target line down
    l.place(
        "sections",
        DropTarget::NewLine {
            area: Bottom,
            line: 1,
        },
    );
    assert_eq!(l.lines[Bottom.index()], vec![line(&["sections"])]);
    l.place("main", DropTarget::NewLine { area: Top, line: 2 });
    let top = &l.lines[Top.index()];
    assert_eq!(top[0], line(&["modify"]));
    assert_eq!(top[1], line(&["view", "views"]));
    assert_eq!(top[2], line(&["main"]));
    l.place("modify", into(Top, 2, 1));
    let top = &l.lines[Top.index()];
    assert_eq!(top.len(), 2);
    assert_eq!(top[1], line(&["main", "modify"]));
    // a bar alone on its line dropped into that same line stays put
    l.place("panels", into(Right, 0, 0));
    assert_eq!(l.lines[Right.index()], vec![line(&["panels"])]);
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
        "views",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    assert_eq!(MainWindow::new().toolbar_area("views"), Some(Bottom));
    win.place_toolbar("views", DropTarget::Float { pos: (5.0, 6.0) });
    let win = MainWindow::new();
    assert_eq!(win.toolbar_area("views"), None);
    assert_eq!(win.floating_toolbars()[0].0 .0, "views");
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
            "Modify",
            "Annotate",
            "Sections",
            "Solids",
            "View",
            "Standard Views",
            "Panels",
            "",
            "Reset toolbar layout"
        ]
    );
    assert!(menu[..9].iter().flatten().all(|e| e.2 == Some(true)));
    let reset = menu.last().unwrap().as_ref().unwrap();
    assert_eq!((reset.0.as_str(), reset.2), ("reset_toolbar_layout", None));
    win.trigger("toolbar:solids");
    assert!(!win.toolbar_layout.is_shown("solids"));
    let solids = toolbars_menu(&win)
        .into_iter()
        .flatten()
        .find(|e| e.0 == "toolbar:solids")
        .unwrap();
    assert_eq!(solids.2, Some(false));
    win.trigger("toolbar:solids");
    assert!(win.toolbar_layout.is_shown("solids"));
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
        "views",
        DropTarget::NewLine {
            area: Bottom,
            line: 0,
        },
    );
    win.place_toolbar("draw", DropTarget::Float { pos: (50.0, 60.0) });
    win.place_toolbar("main", into(Right, 0, 0));
    win.toggle_toolbar("annotate");
    win.toggle_toolbar("solids");
    assert_ne!(win.toolbar_layout, ToolbarLayout::factory());

    assert!(win.trigger("reset_toolbar_layout"));
    assert_eq!(win.toolbar_layout, ToolbarLayout::factory());
    let shown: Vec<&String> = known
        .iter()
        .filter(|id| win.toolbar_layout.is_shown(id))
        .collect();
    assert_eq!(shown, ["main", "draw", "modify", "sections", "view"]);
    assert!(win.floating_toolbars().is_empty());
    assert_eq!(
        ids(&win, Top),
        ["main", "modify", "view", "sections", "draw"]
    );
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
        "views",
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
/// (0..200) and modify (200..400) from y 30; a left column 40 pt wide
/// holding draw from y 70; empty bottom (above a status bar at 680) and
/// right areas.
fn geometry() -> (ToolbarLayout, LayoutGeometry) {
    let mut l = two_areas();
    l.lines[Top.index()] = vec![line(&["main", "modify"])];
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
            ("modify".into(), Rect::new(200.0, 30.0, 400.0, 70.0)),
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
