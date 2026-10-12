// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every menu entry keeps a check-mark column on its left: the labels of
//! a checked entry, an unchecked one, a plain one and a submenu all start
//! at the same x, the same as in a menu with no checkable entry.

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Pos2, RawInput, Rect, Response};
use xtrude_app::shell::menu_bar::menu_buttons;
use xtrude_app::shell::menu_item;

/// What one menu entry is.
#[derive(Clone, Copy)]
enum Kind {
    Checked,
    Unchecked,
    Plain,
    Submenu,
}

/// The two menus: "Mixed" holds one entry of each kind, "Plain" only
/// plain entries.
const MENUS: [&[(&str, Kind)]; 2] = [
    &[
        ("Checked entry", Kind::Checked),
        ("Unchecked entry", Kind::Unchecked),
        ("Plain entry", Kind::Plain),
        ("Submenu entry", Kind::Submenu),
    ],
    &[("First plain", Kind::Plain), ("Second plain", Kind::Plain)],
];

/// One frame of the menu bar.
struct Frame {
    /// The open menu's entries: label and button rect.
    entries: Vec<(&'static str, Rect)>,
    /// The menu bar's button rects.
    bar: Vec<Rect>,
    /// What was painted.
    shapes: Vec<ClippedShape>,
    /// In the open menu: the button padding, the check column's width and
    /// the gap after it.
    menu_spacing: f32,
}

/// Runs one frame with `events`.
fn frame(ctx: &egui::Context, events: Vec<Event>) -> Frame {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    };
    let titles = ["Mixed", "Plain"].map(String::from).to_vec();
    let mut entries = Vec::new();
    let mut bar_rects = Vec::new();
    let mut menu_spacing = 0.0;
    let mut output = ctx.run_ui(input, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            let bar = menu_buttons(ui, &titles, |ui, index| {
                menu_spacing = ui.spacing().button_padding.x
                    + menu_item::check_column_width(ui)
                    + ui.spacing().icon_spacing;
                for &(text, kind) in MENUS[index] {
                    let response: Response = match kind {
                        Kind::Checked => ui.add(menu_item::entry(ui, text, true)),
                        Kind::Unchecked | Kind::Plain => ui.add(menu_item::entry(ui, text, false)),
                        Kind::Submenu => {
                            menu_item::submenu(ui, text, |ui| ui.label("inside")).response
                        }
                    };
                    entries.push((text, response.rect));
                }
            });
            bar_rects = bar.buttons.iter().map(|b| b.rect).collect();
        });
    });
    output.textures_delta.clear();
    Frame {
        entries,
        bar: bar_rects,
        shapes: output.shapes,
        menu_spacing,
    }
}

/// The left x of the painted text `text`.
fn text_left(shapes: &[ClippedShape], text: &str) -> Option<f32> {
    fn find(shape: &Shape, text: &str) -> Option<f32> {
        match shape {
            Shape::Text(t) if t.galley.text() == text => Some(t.pos.x + t.galley.rect.min.x),
            Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    shapes.iter().find_map(|c| find(&c.shape, text))
}

/// The labels of an open menu.
struct Labels {
    /// Each entry's label x, relative to the entry's left edge.
    offsets: Vec<f32>,
    /// The menu's button padding, check column and gap.
    menu_spacing: f32,
}

/// Opens menu `index` with a click on its title and reads its labels.
fn labels(index: usize) -> Labels {
    let ctx = egui::Context::default();
    let at = frame(&ctx, Vec::new()).bar[index].center();
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, vec![Event::PointerMoved(at)]);
    frame(&ctx, vec![button(true)]);
    frame(&ctx, vec![button(false)]);
    // move away so no entry is hovered (a hovered submenu would open)
    let open = frame(&ctx, vec![Event::PointerMoved(Pos2::new(700.0, 500.0))]);
    assert_eq!(
        open.entries.len(),
        MENUS[index].len(),
        "menu {index} is open"
    );
    let offsets = open
        .entries
        .iter()
        .map(|&(text, rect)| {
            let x = text_left(&open.shapes, text).unwrap_or_else(|| panic!("{text} is painted"));
            x - rect.left()
        })
        .collect();
    Labels {
        offsets,
        menu_spacing: open.menu_spacing,
    }
}

#[test]
fn every_label_starts_after_the_check_column() {
    let mixed = labels(0).offsets;
    let plain = labels(1).offsets;
    for (offset, (text, _)) in mixed.iter().zip(MENUS[0]) {
        assert!(
            (offset - mixed[0]).abs() < 0.01,
            "{text} starts at {offset}, not {}",
            mixed[0]
        );
    }
    for offset in &plain {
        assert!(
            (offset - mixed[0]).abs() < 0.01,
            "plain menu: {offset} vs {}",
            mixed[0]
        );
    }
}

#[test]
fn the_label_follows_the_padding_the_column_and_the_gap() {
    let plain = labels(1);
    assert!(plain.menu_spacing > 0.0);
    for offset in &plain.offsets {
        assert!(
            (offset - plain.menu_spacing).abs() < 0.5,
            "label at {offset}, expected {}",
            plain.menu_spacing
        );
    }
}
