// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window ▸ Preferences gathers the scattered settings. OK writes and flushes;
//! Cancel touches nothing; the rest-of-model mode goes through the viewport.
//! The dialog is the toolkit-neutral `Preferences` model (the shell only edits
//! its fields); the settings live in the memory store.

use crate::common;
use xtrude_app::viewport::RestMode;

use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::theme;
use xtrude_app::window::preferences::{
    save_toolbar_icon_px, toolbar_icon_px, Preferences, PrefsField, PrefsPage,
};

use common::v3;
use xtrude_app::window::ScriptedDialogs;
use xtrude_model::section::SectionPlane;
use xtrude_model::settings;

#[test]
fn test_ok_writes_the_settings() {
    let mut win = common::window();
    let mut dlg = Preferences::load(&win);
    dlg.obj_unit = "cm".into();
    dlg.dxf_unit = "m".into();
    dlg.backup = false;
    dlg.invert = true;
    dlg.invert_orbit = true;
    dlg.msaa = 8;
    dlg.accept(&mut win);
    assert_eq!(settings::get_str("import/obj_unit").as_deref(), Some("cm"));
    assert_eq!(settings::get_str("import/dxf_unit").as_deref(), Some("m"));
    assert_eq!(settings::get_str("general/backup").as_deref(), Some("0"));
    assert_eq!(settings::get_str("nav/invert_wheel").as_deref(), Some("1"));
    assert_eq!(
        settings::get_str("nav/invert_orbit_y").as_deref(),
        Some("1")
    );
    assert_eq!(settings::get_i64("display/msaa", 0), 8);
    // The live pieces reach the viewport immediately (the shell rebuilds
    // its render targets when the sample count changes).
    assert!(win.viewport.prefs.invert_wheel);
    assert!(win.viewport.prefs.invert_orbit_y);
    assert_eq!(win.viewport.prefs.msaa, 8);
}

#[test]
fn test_cancel_touches_nothing() {
    let mut win = common::window();
    win.viewport.set_edit_rest_mode(RestMode::Fade);
    let mut dlg = Preferences::load(&win);
    dlg.obj_unit = "ft".into();
    drop(dlg); // Cancel
    assert_eq!(settings::get_str("import/obj_unit"), None); // never written
    assert_eq!(win.viewport.edit_rest_mode(), RestMode::Fade);
}

#[test]
fn test_rest_mode_applies_through_the_viewport() {
    let mut win = common::window();
    win.viewport.set_edit_rest_mode(RestMode::Fade);
    let mut dlg = Preferences::load(&win);
    assert_eq!(dlg.rest, RestMode::Fade); // mirrors the viewport
    dlg.rest = RestMode::Hide;
    dlg.accept(&mut win);
    assert_eq!(win.viewport.edit_rest_mode(), RestMode::Hide);
}

#[test]
fn test_dialog_reloads_saved_values() {
    let win = common::window();
    settings::set("import/dxf_unit", serde_json::json!("in"));
    let dlg = Preferences::load(&win);
    assert_eq!(dlg.dxf_unit, "in");
}

#[test]
fn test_toolbar_icon_size_lives_in_preferences_and_reaches_every_toolbar() {
    let mut win = common::window();
    // migration: the retired toggle reads as 32 px until a size is saved
    settings::set("ui/large_toolbar_icons", serde_json::json!("1"));
    assert_eq!(toolbar_icon_px(), 32);
    let mut dlg = Preferences::load(&win);
    assert_eq!(dlg.icon_px, 32);
    dlg.icon_px = 40;
    dlg.accept(&mut win);
    assert_eq!(settings::get_i64("ui/toolbar_icon_px", 0), 40);
    assert_eq!(settings::get("ui/large_toolbar_icons"), None); // migrated away
    assert_eq!(toolbar_icon_px(), 40);
    save_toolbar_icon_px(24);
    assert_eq!(toolbar_icon_px(), 24);
}

#[test]
fn test_theme_choice_applies_at_once() {
    let mut win = common::window();
    let mut dlg = Preferences::load(&win);
    assert_eq!(dlg.theme, theme::DARK);
    dlg.theme = theme::LIGHT.into();
    dlg.accept(&mut win);
    assert_eq!(theme::saved_theme(), theme::LIGHT);
    // the shell re-resolves the saved theme every frame
    let p = theme::palette_for(None, None);
    assert!(theme::lightness(p.window) > 128);
}

#[test]
fn test_units_page_changes_the_document() {
    // OK on the Units page.
    let mut win = common::window();
    let mut dlg = Preferences::load(&win);
    dlg.unit = "cm".into();
    dlg.decimals = 3;
    dlg.units_for_new = true;
    let v = win.viewport.scene.version;
    dlg.accept(&mut win);
    assert_eq!(win.viewport.scene.units.length, "cm");
    assert_eq!(win.viewport.scene.units.precision, 3);
    assert!(win.viewport.scene.version > v);
    assert_eq!(xtrude_model::units::new_document_units().length, "cm");
}

#[test]
fn test_turning_backups_off_stops_the_bak_copy() {
    let mut win = common::window();
    let mut dlg = Preferences::load(&win);
    dlg.backup = false;
    dlg.accept(&mut win);
    let dir = common::TmpDir::new();
    let path = dir.path().join("doc.igz");
    win.do_save(&path);
    win.do_save(&path); // the second save finds a file to back up
    assert!(path.is_file());
    assert!(!dir.path().join("doc.igz.bak").exists());
}

#[test]
fn test_backups_are_on_by_default() {
    let mut win = common::window();
    let dir = common::TmpDir::new();
    let path = dir.path().join("doc.igz");
    win.do_save(&path);
    win.do_save(&path);
    assert!(dir.path().join("doc.igz.bak").is_file());
}

#[test]
fn test_dont_ask_again_shows_in_preferences_and_back() {
    let mut win = common::window();
    // What the section-name prompt's «Don't ask again» writes:
    settings::set("section/ask_name", serde_json::json!(false));
    let mut dlg = Preferences::load(&win);
    assert!(!dlg.ask_section);
    // Ticked again in Preferences: the prompt asks again.
    dlg.ask_section = true;
    dlg.accept(&mut win);
    assert!(Preferences::load(&win).ask_section);
    let mut dlg = Preferences::load(&win);
    dlg.ask_section = false;
    dlg.accept(&mut win);
    // Unticked in Preferences: the prompt stays away.
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    win.dialogs = Box::new(dialogs.clone());
    let plane = SectionPlane::new(v3(0., 0., 0.), v3(0., 0., 1.), "S 1", "A", true, None);
    let id = plane.id;
    win.viewport.scene.doc.section_planes.push(plane);
    win.prompt_section_name(id);
    assert!(dialogs.borrow().asked.is_empty());
}

#[test]
fn test_pages_in_order() {
    let labels: Vec<&str> = PrefsPage::ALL.iter().map(|p| p.label()).collect();
    assert_eq!(
        labels,
        ["General", "Navigation", "Display", "Import", "Units"]
    );
    for page in PrefsPage::ALL {
        assert!(!page.groups().is_empty(), "{page:?} has no group");
    }
}

#[test]
fn test_every_field_on_exactly_one_page() {
    for field in PrefsField::ALL {
        let shown: usize = PrefsPage::ALL
            .iter()
            .flat_map(|page| page.groups())
            .map(|g| g.fields.iter().filter(|f| **f == field).count())
            .sum();
        assert_eq!(shown, 1, "{field:?} is shown {shown} times");
    }
}

#[test]
fn test_graphics_server_on_display_and_mouse_on_navigation() {
    let page_of = |field| {
        PrefsPage::ALL
            .into_iter()
            .find(|page| page.groups().iter().any(|g| g.fields.contains(&field)))
    };
    assert_eq!(page_of(PrefsField::Platform), Some(PrefsPage::Display));
    assert_eq!(page_of(PrefsField::Msaa), Some(PrefsPage::Display));
    assert_eq!(page_of(PrefsField::NavStyle), Some(PrefsPage::Navigation));
    assert_eq!(page_of(PrefsField::Theme), Some(PrefsPage::General));
    assert_eq!(page_of(PrefsField::ObjUnit), Some(PrefsPage::Import));
    assert_eq!(page_of(PrefsField::LengthUnit), Some(PrefsPage::Units));
}
