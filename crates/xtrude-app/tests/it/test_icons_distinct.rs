// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every tool key still draws an icon, the standard views are told apart, the
//! X-ray button does not look like its neighbours, and
//! the Solids tools cursor draws every state. The icons are rendered by
//! tiny-skia at 48 px.

use xtrude_app::shell::icons::{self, Rgba};

const INK: Rgba = Rgba(40, 44, 52, 255);

fn pixels(key: &str) -> Vec<u8> {
    icons::render(key, 1.0, INK)
        .map(|r| r.rgba)
        .unwrap_or_default()
}

#[test]
fn test_every_tool_key_returns_a_drawn_icon() {
    assert!(!icons::KEYS.is_empty(), "no icons registered");
    for key in icons::KEYS {
        let r = icons::render(key, 1.0, INK).unwrap_or_else(|| panic!("{key}"));
        assert!(
            r.rgba.iter().any(|&b| b != 0) || !r.texts.is_empty(),
            "{key} draws nothing"
        );
    }
    assert!(icons::render("no-such-tool", 1.0, INK).is_none());
}

#[test]
fn test_the_standard_views_are_all_different() {
    let views = [
        "view_iso",
        "view_top",
        "view_bottom",
        "view_front",
        "view_back",
        "view_right",
        "view_left",
    ];
    let drawn: std::collections::HashSet<Vec<u8>> = views.iter().map(|k| pixels(k)).collect();
    assert_eq!(drawn.len(), views.len(), "two views draw the same");
}

#[test]
fn test_xray_differs_from_projection_and_styles() {
    let xray = pixels("xray");
    assert!(xray.iter().any(|&b| b != 0));
    for other in ["projection", "styles", "view_iso"] {
        assert_ne!(xray, pixels(other), "xray draws like {other}");
    }
}

#[test]
fn test_the_solid_tools_are_all_different() {
    let keys = [
        "merge_solids",
        "solid_union",
        "solid_subtract",
        "solid_trim",
        "solid_intersect",
        "solid_split",
    ];
    let drawn: std::collections::HashSet<Vec<u8>> = keys.iter().map(|k| pixels(k)).collect();
    assert_eq!(drawn.len(), keys.len());
}

#[test]
fn test_the_solid_cursor_draws_every_state() {
    for state in ["solid_no", "solid_1", "solid_2"] {
        let (r, _) = icons::cursor(state, false, INK, 1.0).unwrap_or_else(|| panic!("{state}"));
        assert!(r.rgba.iter().any(|&b| b != 0), "{state}");
    }
}

/// The model tree tells a group from a component by its row icon, drawn
/// at the size of a line of text.
#[test]
fn test_the_model_tree_icons_tell_groups_from_components() {
    let at = |key: &str| {
        icons::render(key, 16.0 / icons::PX, INK)
            .map(|r| r.rgba)
            .unwrap_or_default()
    };
    let (group, component) = (at("tree_group"), at("tree_component"));
    assert!(
        group.iter().any(|&b| b != 0),
        "the group icon draws nothing"
    );
    assert_ne!(group, component);
}
