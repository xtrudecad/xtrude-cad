// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! One toolbar icon size for every toolbar of the window,
//! and the toolbar layout around it. The shell reads the size every frame and
//! lays the toolbars out from the window's model; these tests drive that
//! model. The bars wrap rather than overflow.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::TmpDir;
use xtrude_app::window::preferences::{save_toolbar_icon_px, toolbar_icon_px};
use xtrude_app::window::{MenuItem, ScriptedDialogs, ToolbarArea};

#[test]
fn test_the_size_reaches_every_toolbar_live() {
    let _win = common::window();
    // every toolbar is drawn at `toolbar_icon_px()`, read each frame
    save_toolbar_icon_px(40);
    assert_eq!(toolbar_icon_px(), 40);
    save_toolbar_icon_px(20); // live
    assert_eq!(toolbar_icon_px(), 20);
}

#[test]
fn test_the_main_toolbar_starts_with_save() {
    let mut win = common::window();
    let main = &win
        .toolbars
        .iter()
        .find(|(id, _, _)| id == "main")
        .unwrap()
        .2;
    assert_eq!(main[0], "save_tb");
    assert_eq!(win.actions.get("save_tb").unwrap().text, "Save");
    assert_eq!(main[1], "select");
    // it triggers the same save as the menu
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    dialogs.borrow_mut().save_paths.push(None);
    win.trigger("save_tb");
    assert!(dialogs.borrow().asked.iter().any(|(k, _)| k == "save"));
}

#[test]
fn test_simplify_mesh_is_only_in_the_edit_menu() {
    let win = common::window();
    assert!(win
        .toolbars
        .iter()
        .all(|(_, _, ids)| !ids.iter().any(|i| i == "simplify_mesh")));
    let edit = &win
        .menus
        .iter()
        .find(|(t, _)| t.replace('&', "") == "Edit")
        .unwrap()
        .1;
    fn has(items: &[MenuItem], id: &str) -> bool {
        items.iter().any(|i| match i {
            MenuItem::Action(a) => a == id,
            MenuItem::Menu(_, sub) => has(sub, id),
            _ => false,
        })
    }
    assert!(has(edit, "simplify_mesh"));
}

#[test]
fn test_stl_simplification_defaults_to_principal_planes() {
    let tmp = TmpDir::new();
    let mut win = common::window();
    // an old boolean setting does not matter: the mode defaults to principal
    xtrude_model::settings::set("import/stl_simplify", serde_json::json!("true"));
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    dialogs
        .borrow_mut()
        .open_paths
        .push(Some(tmp.join("sample.stl")));
    dialogs.borrow_mut().stl.push(None); // rejected
    win.trigger("import:stl");
    assert_eq!(dialogs.borrow().stl_modes, vec![1]); // «Merge coplanar triangles on XY, XZ and YZ»
}

#[test]
fn test_a_fresh_install_gets_the_factory_layout_and_normal_icons() {
    let win = common::window();
    assert_eq!(toolbar_icon_px(), 24); // normal, since 14-09
    assert_eq!(win.toolbar_area("annotate"), Some(ToolbarArea::Left));
    for name in ["main", "modify", "view", "sections", "views", "draw"] {
        assert!(win.toolbars.iter().any(|(id, _, _)| id == name), "{name}");
        assert_eq!(win.toolbar_area(name), Some(ToolbarArea::Top), "{name}");
    }
}

#[test]
fn test_clean_screen_shows_an_exit_button_at_the_top_right() {
    // The shell hides the menus, toolbars, trays and status bar while the
    // action is checked and floats «Exit clean screen» 12 px from the
    // viewport's top-right; the button triggers the action again.
    let mut win = common::window();
    win.trigger("clean_screen");
    assert!(win.actions.get("clean_screen").unwrap().checked);
    win.trigger("clean_screen"); // the exit button
    assert!(!win.actions.get("clean_screen").unwrap().checked);
}

#[test]
fn test_the_sidebar_handle_sits_on_the_resize_line_and_folds_the_trays() {
    // The shell draws the handle on the line between the viewport and the
    // trays, half-way down; a click folds the trays and unfolds them.
    let mut win = common::window();
    assert!(win.sidebar_visible() && win.tray_shown("tray_properties"));
    win.set_sidebar_visible(false); // fold
    assert!(!win.sidebar_visible());
    assert!(!win.actions.get("sidebar").unwrap().checked);
    win.trigger("sidebar"); // unfold (Ctrl+F5)
    assert!(win.sidebar_visible() && win.tray_shown("tray_properties"));
}

#[test]
fn test_the_default_is_normal_on_any_screen_and_a_choice_wins() {
    let _win = common::window();
    assert_eq!(toolbar_icon_px(), 24);
    save_toolbar_icon_px(40);
    assert_eq!(toolbar_icon_px(), 40);
}
