// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Parallel Projection is checked exactly while the camera is parallel,
//! whatever switched it: the action, the viewport's own key, an opened
//! document's camera, a new document or a saved view.

use crate::common;

use common::{window, TmpDir};
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tray::saved_views::SavedViews;
use xtrude_app::window::MainWindow;

fn checked(w: &MainWindow) -> bool {
    w.actions.get("toggle_projection").unwrap().checked
}

#[test]
fn test_a_new_window_is_in_perspective_and_unchecked() {
    let w = window();
    let a = w.actions.get("toggle_projection").unwrap();
    assert_eq!(a.text, "Parallel Projection");
    assert!(a.checkable);
    assert!(!a.checked);
    assert!(w.viewport.camera.perspective);
}

#[test]
fn test_the_action_checks_and_unchecks() {
    let mut w = window();
    w.trigger("toggle_projection");
    assert!(checked(&w) && !w.viewport.camera.perspective);
    w.trigger("toggle_projection");
    assert!(!checked(&w) && w.viewport.camera.perspective);
}

#[test]
fn test_shift_p_checks_it() {
    let mut w = window();
    w.key_press(&KeyEvent::new(Key::Char('P'), "", Modifiers::SHIFT));
    assert!(checked(&w) && !w.viewport.camera.perspective);
}

#[test]
fn test_the_viewports_own_key_is_followed() {
    let mut w = window();
    // With P left to no action, the key reaches the viewport, which
    // switches the projection without the window's action.
    w.actions.get_mut("pushpull").unwrap().shortcuts.clear();
    w.key_press(&KeyEvent::new(Key::Char('P'), "", Modifiers::NONE));
    assert!(!w.viewport.camera.perspective);
    assert!(checked(&w));
    w.key_press(&KeyEvent::new(Key::Char('P'), "", Modifiers::NONE));
    assert!(w.viewport.camera.perspective);
    assert!(!checked(&w));
}

#[test]
fn test_a_document_saved_in_parallel_opens_checked_and_new_unchecks() {
    let dir = TmpDir::new();
    let mut w = window();
    w.mark_saved();
    w.viewport.camera.perspective = false;
    let path = dir.path().join("parallel.xtrude");
    w.do_save(&path);
    w.trigger("new");
    assert!(!checked(&w) && w.viewport.camera.perspective);
    assert!(w.open_path(&path));
    assert!(checked(&w) && !w.viewport.camera.perspective);
    w.mark_saved();
    w.trigger("new");
    assert!(!checked(&w) && w.viewport.camera.perspective);
}

#[test]
fn test_a_saved_view_in_parallel_checks_it() {
    let mut w = window();
    let mut p = SavedViews::default();
    w.trigger("toggle_projection");
    p.add(&mut w);
    let id = p.rows[0].0;
    w.trigger("toggle_projection");
    assert!(!checked(&w));
    p.activate(&mut w, id);
    assert!(!w.viewport.camera.perspective);
    assert!(checked(&w));
}
