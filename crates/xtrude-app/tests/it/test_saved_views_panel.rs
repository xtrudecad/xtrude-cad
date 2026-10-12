// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Saved views tray section: «+ View» names each new view «View N»,
//! and recalling or updating one says so by name.

use crate::common;

use common::window;
use xtrude_app::tray::saved_views::SavedViews;
use xtrude_app::tray::SECTIONS;

#[test]
fn test_the_section_is_titled_saved_views() {
    let (_, title) = SECTIONS.iter().find(|(k, _)| *k == "saved_views").unwrap();
    assert_eq!(*title, "Saved views");
    assert!(SECTIONS.iter().all(|(k, _)| *k != "scenes"));
}

#[test]
fn test_new_views_are_numbered_views() {
    let mut w = window();
    let mut p = SavedViews::default();
    p.add(&mut w);
    p.add(&mut w);
    let names: Vec<&str> = p.rows.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["View 1", "View 2"]);
}

#[test]
fn test_recall_and_update_name_the_view() {
    let mut w = window();
    let mut p = SavedViews::default();
    p.add(&mut w);
    let id = p.rows[0].0;
    p.activate(&mut w, id);
    let shown =
        |w: &xtrude_app::window::MainWindow| w.status_message.as_ref().map(|m| m.text.clone());
    assert_eq!(shown(&w).as_deref(), Some("View 'View 1'"));
    p.current = Some(id);
    p.update(&mut w);
    assert_eq!(shown(&w).as_deref(), Some("View 'View 1' updated"));
}
