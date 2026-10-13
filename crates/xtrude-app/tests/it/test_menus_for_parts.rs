// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The menu bar for parts: File, Edit, View, Draw, Model, Inspect,
//! Extensions, Window, Help, each with its entries, and every action in a
//! menu.

use crate::common;

use xtrude_app::window::{MainWindow, MenuItem};

/// A menu's entries as text: an action by its id, a submenu as
/// `"Title ▸"` followed by its entries indented, a separator as `"-"`.
fn entries(items: &[MenuItem]) -> Vec<String> {
    let mut out = Vec::new();
    for i in items {
        match i {
            MenuItem::Action(id) => out.push(id.clone()),
            MenuItem::Separator => out.push("-".into()),
            MenuItem::Menu(title, sub) => {
                out.push(format!("{title} ▸"));
                out.extend(entries(sub).into_iter().map(|e| format!("  {e}")));
            }
            MenuItem::Dynamic(name) => out.push(format!("<{name}>")),
        }
    }
    out
}

fn menu(w: &MainWindow, title: &str) -> Vec<String> {
    let (_, items) = w
        .menus
        .iter()
        .find(|(t, _)| t == title)
        .unwrap_or_else(|| panic!("no {title} menu"));
    entries(items)
}

/// Every action id a menu reaches, submenus included.
fn all_ids(items: &[MenuItem], out: &mut Vec<String>) {
    for i in items {
        match i {
            MenuItem::Action(id) => out.push(id.clone()),
            MenuItem::Menu(_, sub) => all_ids(sub, out),
            _ => {}
        }
    }
}

#[test]
fn test_the_menu_bar_follows_the_work() {
    let w = common::window();
    let titles: Vec<&str> = w.menus.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        titles,
        [
            "File",
            "Edit",
            "View",
            "Draw",
            "Model",
            "Inspect",
            "Extensions",
            "Window",
            "Help"
        ]
    );
}

#[test]
fn test_file_exports_stl_first() {
    let w = common::window();
    let file = menu(&w, "File");
    let at = file.iter().position(|e| e == "Export ▸").unwrap();
    assert_eq!(
        file[at + 1..],
        [
            "  export:stl",
            "  export:3mf",
            "  export:obj",
            "  export:glb",
            "  export:dae",
            "-",
            "quit"
        ]
    );
}

/// No image export: the view on screen is a screenshot away, and the
/// action never saved anything.
#[test]
fn test_no_image_export() {
    let w = common::window();
    assert!(w.actions.get("export:image").is_none());
}

#[test]
fn test_the_edit_menu() {
    let w = common::window();
    assert_eq!(
        menu(&w, "Edit"),
        [
            "undo",
            "redo",
            "repeat_last_command",
            "-",
            "cut",
            "copy",
            "paste",
            "paste_in_place",
            "-",
            "select",
            "select_all",
            "invert_selection",
            "-",
            "make_group",
            "make_component",
            "explode_group",
            "-",
            "hide",
            "Unhide ▸",
            "  unhide_selected",
            "  unhide_last",
            "  unhide_all",
            "delete_guides",
            "-",
            "cancel_tool",
        ]
    );
}

#[test]
fn test_the_view_menu() {
    let w = common::window();
    let view = menu(&w, "View");
    assert_eq!(
        view[..10],
        [
            "Standard Views ▸",
            "  view:top",
            "  view:bottom",
            "  view:front",
            "  view:back",
            "  view:left",
            "  view:right",
            "  view:iso",
            "zoom_extents",
            "zoom_selection",
        ]
    );
    for id in [
        "toggle_projection",
        "Style ▸",
        "Rest of model while editing ▸",
        "show_section_planes",
        "show_section_cuts",
        "section_fill",
        "show_hidden_groups",
        "show_hidden_edges_faces",
    ] {
        assert!(view.iter().any(|e| e == id), "{id}");
    }
    assert_eq!(
        view[view.len() - 4..],
        ["nav:orbit", "nav:pan", "nav:zoom", "nav:zoom_window"]
    );
}

#[test]
fn test_the_draw_menu() {
    let w = common::window();
    assert_eq!(
        menu(&w, "Draw"),
        [
            "line",
            "freehand",
            "Arcs ▸",
            "  arc",
            "  arc3",
            "  center_arc",
            "  sector",
            "Shapes ▸",
            "  rectangle",
            "  rotated_rect",
            "  circle",
            "  polygon",
            "-",
            "eraser",
        ]
    );
}

#[test]
fn test_the_model_menu() {
    let w = common::window();
    assert_eq!(
        menu(&w, "Model"),
        [
            "pushpull",
            "sweep",
            "offset",
            "fillet",
            "text3d",
            "-",
            "move",
            "rotate",
            "scale",
            "flip",
            "-",
            "Solids ▸",
            "  solid_union",
            "  solid_subtract",
            "  solid_trim",
            "  solid_intersect",
            "  solid_split",
            "  merge_solids",
            "Add Crossing Edges ▸",
            "  intersect:model",
            "  intersect:selection",
            "  intersect:context",
            "split_into_pieces",
            "-",
            "Mesh repair ▸",
            "  reverse_faces",
            "  orient_faces",
            "  heal_overlaps",
            "  rebuild_planar",
            "  simplify_mesh",
            "-",
            "paint",
            "eyedropper",
        ]
    );
}

#[test]
fn test_the_inspect_menu() {
    let w = common::window();
    assert_eq!(
        menu(&w, "Inspect"),
        [
            "measure",
            "protractor",
            "section",
            "-",
            "solid_inspector",
            "model_info",
            "-",
            "Annotations ▸",
            "  dimension",
            "  text",
        ]
    );
}

#[test]
fn test_the_window_menu_keeps_the_layout_and_preferences() {
    let w = common::window();
    assert_eq!(
        menu(&w, "Window"),
        [
            "Panels ▸",
            "  <panels>",
            "Toolbars ▸",
            "  <toolbars>",
            "-",
            "clean_screen",
            "sidebar",
            "-",
            "preferences",
        ]
    );
}

#[test]
fn test_every_action_is_in_a_menu() {
    let w = common::window();
    let mut in_menus = Vec::new();
    for (_, items) in &w.menus {
        all_ids(items, &mut in_menus);
    }
    // each id once: a menu entry has one home
    let mut sorted = in_menus.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), in_menus.len(), "an action in two menus");
    // reached another way: the context menu, the Main bar's copy of
    // Save, the Toolbars and Panels lists, the Extensions menu once an
    // extension failed to load
    let elsewhere = [
        "change_axes",
        "save_tb",
        "reset_toolbar_layout",
        "reset_panel_layout",
        "ext_failed",
        "ext_failures",
    ];
    let missing: Vec<&str> = w
        .actions
        .iter()
        .map(|a| a.id.as_str())
        .filter(|id| !in_menus.iter().any(|m| m == id) && !elsewhere.contains(id))
        .collect();
    assert_eq!(missing, Vec::<&str>::new());
}
