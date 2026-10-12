// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! For the one tray the app docks: the Window menu can always bring it
//! back, a closed tray stays closed at the next start, and a folded sidebar
//! opens unfolded.

use crate::common;

use common::window;
use xtrude_app::window::MainWindow;

#[test]
fn test_the_window_menu_can_bring_back_every_tray() {
    let mut w = window();
    for (name, _) in w.trays() {
        w.set_tray_shown(&name, false);
        assert!(!w.tray_shown(&name));
        // any of its sections' entries in Window ▸ Panels
        w.trigger("tray_section:model_tree");
        assert!(w.tray_shown(&name), "{name}");
    }
}

#[test]
fn test_every_tray_opens_at_start_up_unless_it_was_closed() {
    let mut w = window();
    w.set_tray_shown("tray_properties", false); // closed
    assert!(!w.tray_shown("tray_properties"));
    let mut again = MainWindow::new(); // the next start: still closed
    assert!(!again.tray_shown("tray_properties"));
    again.trigger("tray_section:layers");
    assert!(MainWindow::new().tray_shown("tray_properties"));
}

#[test]
fn test_a_folded_sidebar_opens_unfolded() {
    let mut w = window();
    w.set_sidebar_visible(false);
    assert!(!w.sidebar_visible());
    assert!(MainWindow::new().sidebar_visible());
}
