// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The menu bar switches menus on hover: while one bar menu is open,
//! hovering another entry opens that one in its place. With none open,
//! hovering opens nothing; a click opens a menu and clicking its title
//! again closes it.

use egui::{Event, Id, Popup, Pos2, RawInput, Rect};
use xtrude_app::shell::menu_bar::menu_buttons;

/// A headless egui context driving a menu bar of three entries.
struct Bar {
    ctx: egui::Context,
    titles: Vec<String>,
    /// The entries' rects, as last laid out.
    rects: Vec<Rect>,
    /// The entries' popup ids.
    popups: Vec<Id>,
}

impl Bar {
    fn new() -> Self {
        let mut bar = Bar {
            ctx: egui::Context::default(),
            titles: ["File", "Edit", "View"].map(String::from).to_vec(),
            rects: Vec::new(),
            popups: Vec::new(),
        };
        bar.frame(Vec::new());
        bar
    }

    /// Runs one frame with `events`; returns the menu open after it.
    fn frame(&mut self, events: Vec<Event>) -> Option<usize> {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            ..RawInput::default()
        };
        let titles = self.titles.clone();
        let mut open = None;
        let mut rects = Vec::new();
        let mut popups = Vec::new();
        self.ctx
            .run_ui(input, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let bar = menu_buttons(ui, &titles, |ui, index| {
                        ui.label(format!("item of {index}"));
                    });
                    rects = bar.buttons.iter().map(|b| b.rect).collect();
                    popups = bar.buttons.iter().map(Popup::default_response_id).collect();
                    open = bar.open;
                });
            })
            .textures_delta
            .clear();
        self.rects = rects;
        self.popups = popups;
        open
    }

    /// Moves the pointer onto entry `index` (two frames, so egui sees the
    /// hover) and returns the menu open after it.
    fn hover(&mut self, index: usize) -> Option<usize> {
        let at = self.rects[index].center();
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(Vec::new())
    }

    /// Clicks entry `index` and returns the menu open after it.
    fn click(&mut self, index: usize) -> Option<usize> {
        let at = self.rects[index].center();
        self.frame(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.frame(Vec::new())
    }

    /// Which entries' popups egui's memory holds open.
    fn popups_open(&self) -> Vec<bool> {
        self.popups
            .iter()
            .map(|&id| Popup::is_id_open(&self.ctx, id))
            .collect()
    }
}

#[test]
fn hovering_another_entry_switches_the_open_menu() {
    let mut bar = Bar::new();
    assert_eq!(bar.click(0), Some(0), "a click opens File");
    assert_eq!(bar.hover(1), Some(1), "hovering Edit opens it");
    // the frame after the switch draws Edit's popup and File's is closed
    assert_eq!(bar.frame(Vec::new()), Some(1));
    assert_eq!(bar.popups_open(), [false, true, false]);
    assert_eq!(bar.hover(2), Some(2), "and on to View");
    assert_eq!(bar.hover(0), Some(0), "and back to File");
}

#[test]
fn hovering_with_no_menu_open_opens_nothing() {
    let mut bar = Bar::new();
    for index in 0..3 {
        assert_eq!(bar.hover(index), None);
    }
    assert_eq!(bar.popups_open(), [false; 3]);
}

#[test]
fn leaving_the_entries_keeps_the_open_menu() {
    let mut bar = Bar::new();
    assert_eq!(bar.click(1), Some(1));
    // the gap below the bar, then far away: no entry hovered
    let below = bar.rects[1].center_bottom() + egui::vec2(0.0, 30.0);
    bar.frame(vec![Event::PointerMoved(below)]);
    assert_eq!(bar.frame(Vec::new()), Some(1));
    bar.frame(vec![Event::PointerMoved(Pos2::new(700.0, 500.0))]);
    assert_eq!(bar.frame(Vec::new()), Some(1));
}

#[test]
fn clicking_the_open_entry_closes_its_menu() {
    let mut bar = Bar::new();
    assert_eq!(bar.click(0), Some(0));
    assert_eq!(bar.click(0), None);
    assert_eq!(bar.popups_open(), [false; 3]);
    assert_eq!(bar.click(2), Some(2), "a click still opens another");
}
