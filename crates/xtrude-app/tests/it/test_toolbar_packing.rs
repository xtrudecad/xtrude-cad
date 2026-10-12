// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A left column packs its bars and the top keeps its order. The egui
//! shell lays the bars out itself, packed, from `toolbars_in` (no saved
//! lengths can strand them), so the order is what is left to check.

use crate::common;

use serde_json::json;
use xtrude_app::window::{MainWindow, ToolbarArea};
use xtrude_model::settings::keys;

#[test]
fn test_a_left_column_packs_and_the_top_keeps_its_order() {
    let _win = common::window();
    xtrude_model::settings::set(
        keys::TOOLBAR_LAYOUT.name(),
        json!({
            "top": [["main", "modify", "view", "sections", "views", "solids", "panels"]],
            "left": [["draw", "annotate"]],
        }),
    );
    let win = MainWindow::new();
    let left: Vec<String> = win
        .toolbars_in(ToolbarArea::Left)
        .into_iter()
        .map(|b| b.0)
        .collect();
    assert_eq!(left, ["draw", "annotate"]);
    let top: Vec<String> = win
        .toolbars_in(ToolbarArea::Top)
        .into_iter()
        .map(|b| b.0)
        .collect();
    let at = |n: &str| top.iter().position(|t| t == n).unwrap();
    assert!(at("view") < at("sections")); // the saved order
}
