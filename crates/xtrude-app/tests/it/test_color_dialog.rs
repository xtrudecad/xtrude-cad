// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every colour picker is the program's own dialog (on GNOME the native one,
//! opened from a right-click menu, came back without the colour chosen).
//! Every picker goes through `Dialogs::get_color`, which
//! the shell answers with its own egui picker.

use xtrude_app::shell::dialogs::{Answer, Question, ShellDialogs};
use xtrude_app::window::status_bar::StatusBar;
use xtrude_app::window::Dialogs;

#[test]
fn test_the_picker_asks_for_qts_own_dialog() {
    let mut d = ShellDialogs::default();
    // first call: the question is put up in the shell's own modal
    assert_eq!(d.get_color("Colour", [0.0, 0.0, 0.0]), None);
    assert!(matches!(d.pending, Some(Question::Color { .. })));
    // the modal's answer comes back on the replay
    d.pending = None;
    d.answers.push_back(Answer::Color(Some([
        10.0 / 255.0,
        20.0 / 255.0,
        30.0 / 255.0,
    ])));
    let c = d.get_color("Colour", [0.0, 0.0, 0.0]).expect("a colour");
    assert_eq!((c[0] * 255.0).round(), 10.0);
}

#[test]
fn test_no_panel_opens_the_desktops_picker_directly() {
    // The only picker widget is the shell's modal for `Question::Color`.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") && !p.ends_with("shell/mod.rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                assert!(!text.contains("color_picker::"), "{}", p.display());
            }
        }
    }
}

#[test]
fn test_a_menu_entrys_description_shows_in_the_status_bar() {
    // hovering a menu entry shows its description; leaving the menu
    // gives the standing text back.
    let mut bar = StatusBar::default();
    bar.show_message("standing hint", 0, 0.0);
    bar.show_tip(Some("Take back the last change to the model."));
    assert_eq!(
        bar.current_message(),
        "Take back the last change to the model."
    );
    bar.show_tip(Some(""));
    assert_eq!(bar.current_message(), "standing hint");
}
