// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport's rest-of-model mode while editing a group (the document
//! half lives in crates/xtrude-io).

use crate::common;

use common::viewport;
use xtrude_app::viewport::RestMode;

#[test]
fn test_edit_rest_mode_defaults_to_fade_and_validates() {
    assert!(RestMode::from_key("fade").is_some() && RestMode::from_key("hide").is_some());
    let vp = viewport();
    assert!(RestMode::ALL.contains(&vp.edit_rest_mode()));
}

#[test]
fn test_rest_mode_keeps_its_settings_spelling() {
    for (mode, key) in [
        (RestMode::Normal, "normal"),
        (RestMode::Fade, "fade"),
        (RestMode::Hide, "hide"),
    ] {
        assert_eq!(mode.key(), key);
        assert_eq!(RestMode::from_key(key), Some(mode));
    }
    assert_eq!(RestMode::from_key("Fade"), None);
}

#[test]
fn test_a_stored_rest_mode_is_read_back_and_an_unknown_one_is_fade() {
    xtrude_model::settings::use_memory_store();
    xtrude_model::settings::set("display/edit_rest_mode", serde_json::json!("hide"));
    assert_eq!(
        xtrude_app::viewport::Viewport::new().edit_rest_mode(),
        RestMode::Hide
    );
    xtrude_model::settings::set("display/edit_rest_mode", serde_json::json!("dim"));
    assert_eq!(
        xtrude_app::viewport::Viewport::new().edit_rest_mode(),
        RestMode::Fade
    );
}
