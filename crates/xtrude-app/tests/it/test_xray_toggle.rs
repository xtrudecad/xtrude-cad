// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Alt+X switches X-ray on and, pressed again, returns to the style you were
//! in — your tweaks included. Its View bar button is pressed exactly while
//! the faces draw in X-ray, however that came about; the menu entry is not
//! checkable and shows no check mark.

use crate::common;

use common::{window, TmpDir};
use xtrude_app::tray::saved_views::SavedViews;
use xtrude_app::tray::styles::Styles;
use xtrude_app::window::actions::KeySeq;
use xtrude_app::window::MainWindow;

fn checked(w: &MainWindow, id: &str) -> bool {
    w.actions.get(id).is_some_and(|a| a.checked)
}

fn set_profiles(w: &mut MainWindow, on: bool) {
    if w.viewport.scene.display_style.profiles != on {
        w.trigger("style_silhouettes");
    }
}

#[test]
fn test_alt_x_is_the_toggle_and_no_other_action_holds_it() {
    let w = window();
    let alt_x = KeySeq::parse("Alt+X").unwrap();
    let holders: Vec<String> = w
        .actions
        .owners(alt_x)
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(holders, vec!["toggle_xray".to_string()]);
}

#[test]
fn test_the_toggle_goes_back_to_the_style_you_left() {
    let mut w = window();
    w.trigger("style:Monochrome");
    set_profiles(&mut w, true);
    w.trigger("toggle_xray");
    assert_eq!(w.viewport.scene.display_style.face_mode, "xray");
    assert!(checked(&w, "style:X-ray"));
    w.trigger("toggle_xray");
    let st = &w.viewport.scene.display_style;
    assert!(st.name == "Monochrome" && st.face_mode == "monochrome");
    assert!(st.profiles); // your tweak came back too
    assert!(checked(&w, "style:Monochrome"));
}

#[test]
fn test_xray_picked_from_the_menu_toggles_out_to_default() {
    let mut w = window();
    w.trigger("style:X-ray");
    w.trigger("toggle_xray");
    assert_eq!(w.viewport.scene.display_style.name, "Default");
}

#[test]
fn test_the_button_is_pressed_while_in_xray() {
    let mut w = window();
    let a = w.actions.get("toggle_xray").unwrap();
    assert!(!a.checkable && !a.checked);
    assert_eq!(a.icon.as_deref(), Some("xray"));
    w.trigger("toggle_xray");
    assert!(checked(&w, "toggle_xray"));
    w.trigger("toggle_xray");
    assert!(!checked(&w, "toggle_xray"));
    assert!(!w.actions.get("toggle_xray").unwrap().checkable);
}

#[test]
fn test_the_button_follows_the_style_menu() {
    let mut w = window();
    w.trigger("style:X-ray");
    assert!(checked(&w, "toggle_xray"));
    w.trigger("style:Default");
    assert!(!checked(&w, "toggle_xray"));
}

#[test]
fn test_the_button_follows_a_style_edited_to_xray() {
    let mut w = window();
    let mut p = Styles::default();
    p.refresh(&w);
    p.edit(&mut w, |s| s.face_mode = "xray".into());
    assert!(checked(&w, "toggle_xray"));
}

#[test]
fn test_the_button_follows_a_saved_view_in_xray() {
    let mut w = window();
    let mut p = SavedViews::default();
    w.trigger("style:X-ray");
    p.add(&mut w);
    let id = p.rows[0].0;
    w.trigger("style:Default");
    assert!(!checked(&w, "toggle_xray"));
    p.activate(&mut w, id);
    assert_eq!(w.viewport.scene.display_style.face_mode, "xray");
    assert!(checked(&w, "toggle_xray"));
}

#[test]
fn test_the_button_follows_a_document_saved_in_xray() {
    let dir = TmpDir::new();
    let mut w = window();
    w.mark_saved();
    w.trigger("style:X-ray");
    let path = dir.path().join("xray.xtrude");
    w.do_save(&path);
    w.trigger("style:Default");
    assert!(!checked(&w, "toggle_xray"));
    w.mark_saved();
    assert!(w.open_path(&path));
    assert!(checked(&w, "toggle_xray"));
}
