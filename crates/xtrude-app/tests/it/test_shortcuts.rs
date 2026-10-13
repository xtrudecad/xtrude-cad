// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! No two actions may hold the same key — the window then fires NEITHER
//! rather than guess one. The camera keys are the usual card's; the
//! tools that yielded keep a Shift+key.

use crate::common;

use std::collections::BTreeMap;

use common::window;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::arc::ArcTool;
use xtrude_app::tools::Tool;
use xtrude_app::window::{KeySeq, MainWindow};

/// `{keys: [names of the actions that ask for them]}`.
fn all_shortcuts(w: &MainWindow) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for a in w.actions.iter() {
        for s in &a.shortcuts {
            out.entry(s.text()).or_default().push(a.text.clone());
        }
    }
    out
}

/// The names of the actions one keystroke fires.
fn press(w: &mut MainWindow, key: Key, mods: Modifiers) -> Vec<String> {
    let ev = KeyEvent::new(key, "", mods);
    let owners: Vec<String> = w
        .actions
        .for_key(&ev)
        .iter()
        .map(|a| a.text.clone())
        .collect();
    w.key_press(&ev);
    if owners.len() == 1 {
        owners
    } else {
        Vec::new()
    }
}

#[test]
fn test_no_action_takes_another_actions_shortcut() {
    let w = window();
    let clash: Vec<String> = all_shortcuts(&w)
        .into_iter()
        .filter(|(_, n)| n.len() > 1)
        .map(|(k, n)| format!("{k} -> {n:?}"))
        .collect();
    assert!(
        clash.is_empty(),
        "shared shortcuts — all DEAD: {}",
        clash.join("; ")
    );
}

#[test]
fn test_the_camera_keys_are_the_usual_ones() {
    let mut w = window();
    for (key, mods, expected) in [
        (Key::Char('O'), Modifiers::NONE, "Orbit"),
        (Key::Char('H'), Modifiers::NONE, "Pan"),
        (Key::Char('Z'), Modifiers::NONE, "Zoom"),
        (Key::Char('Z'), Modifiers::SHIFT, "Zoom Extents"),
        (Key::F(2), Modifiers::NONE, "Zoom Extents"),
    ] {
        assert_eq!(press(&mut w, key, mods), vec![expected.to_string()]);
    }
    assert!(w.is_checked("nav:zoom"));
}

#[test]
fn test_the_two_tools_that_gave_up_their_keys_stay_at_hand() {
    let mut w = window();
    assert_eq!(
        press(&mut w, Key::Char('H'), Modifiers::SHIFT),
        vec!["Protractor"]
    );
    assert_eq!(w.active_tool_key(), Some("protractor"));
    assert_eq!(
        press(&mut w, Key::Char('O'), Modifiers::SHIFT),
        vec!["Center Arc"]
    );
    assert_eq!(w.active_tool_key(), Some("center_arc"));
}

const USUAL_CARD: [(&str, &str); 18] = [
    ("Space", "Select"),
    ("B", "Paint"),
    ("E", "Eraser"),
    ("L", "Line"),
    ("R", "Rectangle"),
    ("C", "Circle"),
    ("A", "Arc"),
    ("M", "Move"),
    ("Q", "Rotate"),
    ("S", "Scale"),
    ("F", "Offset"),
    ("T", "Measure"),
    ("O", "Orbit"),
    ("H", "Pan"),
    ("Z", "Zoom"),
    ("Shift+Z", "Zoom Extents"),
    ("G", "Make Component…"),
    ("P", "Push / Pull"),
];

#[test]
fn test_the_usual_key_card_is_kept() {
    let w = window();
    let all = all_shortcuts(&w);
    for (key, name) in USUAL_CARD {
        assert_eq!(
            all.get(key),
            Some(&vec![name.to_string()]),
            "{key} should be {name:?} as on the card"
        );
    }
}

#[test]
fn test_the_arc_on_a_is_the_bulge_arc() {
    let arc = ArcTool::new();
    assert_eq!(arc.info().shortcut, Some("A"));
    assert_eq!(arc.value_box_caption().as_deref(), Some("Bulge"));
}

#[test]
fn test_p_is_push_pull_and_u_still_works() {
    let mut w = window();
    assert_eq!(
        press(&mut w, Key::Char('P'), Modifiers::NONE),
        vec!["Push / Pull"]
    );
    assert_eq!(
        press(&mut w, Key::Char('U'), Modifiers::NONE),
        vec!["Push / Pull"]
    );
    let all = all_shortcuts(&w);
    assert_eq!(all["P"], vec!["Push / Pull"]);
    assert_eq!(all["U"], vec!["Push / Pull"]);
}

#[test]
fn test_perspective_moved_to_shift_p() {
    let mut w = window();
    let before = w.viewport.camera.perspective;
    assert_eq!(
        press(&mut w, Key::Char('P'), Modifiers::SHIFT),
        vec!["Parallel Projection"]
    );
    assert_ne!(w.viewport.camera.perspective, before);
}

#[test]
fn test_the_toolbar_shows_the_configured_shortcut_not_the_factory_one() {
    let mut w = window();
    let first = |w: &MainWindow, id: &str| {
        w.actions
            .get(id)
            .unwrap()
            .tooltip()
            .split('\n')
            .next()
            .unwrap()
            .to_string()
    };
    assert!(first(&w, "line").ends_with("(L)"));
    w.actions.get_mut("line").unwrap().shortcuts = vec![KeySeq::parse("Ctrl+Alt+L").unwrap()];
    assert!(first(&w, "line").contains("Ctrl+Alt+L"));
    assert!(!first(&w, "line").contains("(L)"));
    w.actions.get_mut("line").unwrap().shortcuts.clear();
    assert!(!first(&w, "line").contains('(')); // no keys, no brackets
    w.actions.get_mut("line").unwrap().shortcuts = vec![KeySeq::parse("L").unwrap()];
    assert!(first(&w, "line").ends_with("(L)"));
    w.actions.get_mut("zoom_extents").unwrap().shortcuts = vec![KeySeq::parse("Ctrl+E").unwrap()];
    assert!(first(&w, "zoom_extents").contains("Ctrl+E"));
}
