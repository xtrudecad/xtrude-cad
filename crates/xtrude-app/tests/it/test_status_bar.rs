// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The standing hint, timed messages, and a long hint that never widens the
//! window. The clock is passed in.

use crate::common;

use xtrude_app::window::status_bar::StatusBar;

#[test]
fn test_a_timed_message_gives_the_standing_hint_back() {
    let mut win = common::window();
    win.activate_tool("line");
    let mut bar = StatusBar::default();
    bar.set_standing(&win.status_hint());
    let standing = bar.current_message().to_string();
    bar.show_message("Selected everything", 50, 10.0);
    assert_eq!(bar.current_message(), "Selected everything");
    bar.tick(10.02);
    assert_eq!(bar.current_message(), "Selected everything"); // still up
                                                              // the hint changing underneath does not cut the flash short
    bar.set_standing(&standing);
    bar.tick(10.06);
    assert_eq!(bar.current_message(), standing); // the hint comes back
}

#[test]
fn test_the_standing_hint_never_widens_the_window() {
    // The message is drawn elided to its share of the bar (egui's
    // `Label::truncate` in a width-capped cell), so the text never sets the
    // window's minimum width; the full text is kept and shown on hover.
    let long_hint = "Select objects. ".repeat(12);
    let mut bar = StatusBar::default();
    bar.show_message(&long_hint, 0, 0.0);
    assert!(bar.current_message().len() > 100);
    assert!(StatusBar::message_max_width(700.0) < 1000.0);
    assert_eq!(bar.current_message(), long_hint); // full text kept
}

#[test]
fn test_a_timed_message_says_when_it_gives_way() {
    // the shell asks for a frame then: a window left alone still gets
    // its standing hint back
    let mut bar = StatusBar::default();
    bar.show_message("Standing", 0, 0.0);
    assert_eq!(bar.next_change(0.0), None);
    bar.show_message("Copied", 2000, 10.0);
    assert_eq!(bar.next_change(10.5), Some(1.5));
    assert_eq!(bar.next_change(13.0), Some(0.0)); // overdue: now
    bar.tick(13.0);
    assert_eq!(bar.next_change(13.0), None);
    assert_eq!(bar.current_message(), "Standing");
}
