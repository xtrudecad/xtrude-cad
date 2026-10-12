// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Menu entries with a check-mark column. Every entry of a menu (an
//! action, a dynamic entry or a submenu) starts with a column as wide as
//! the check mark, which holds `✔` when the entry is checked and stays
//! empty otherwise, so that all the labels of a menu start at the same x.

use egui::{
    Atom, AtomExt as _, Button, InnerResponse, TextStyle, TextWrapMode, Ui, Vec2, WidgetText,
};

/// The check mark drawn in the column of a checked entry.
const CHECK_MARK: &str = "✔";

/// The width of the check column: the check mark's width in the button
/// font. The button puts its usual gap between the column and the label.
pub fn check_column_width(ui: &Ui) -> f32 {
    mark_size(ui).x
}

/// The size of the check mark in the button font.
fn mark_size(ui: &Ui) -> Vec2 {
    WidgetText::from(CHECK_MARK)
        .into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        )
        .size()
}

/// The check column of an entry: the mark when `checked`, else empty space.
fn check_column(ui: &Ui, checked: bool) -> Atom<'static> {
    let atom: Atom<'static> = if checked {
        CHECK_MARK.into()
    } else {
        Atom::default()
    };
    // as tall as the mark too, so that the mark is centred on the label
    atom.atom_size(mark_size(ui))
}

/// A menu entry's button: the check column, then `text`. Add the shortcut
/// text and the enabled state as with any button.
pub fn entry(ui: &Ui, text: &str, checked: bool) -> Button<'static> {
    Button::new((check_column(ui, checked), text.to_owned()))
}

/// A submenu entry: the empty check column, then `title`, with the
/// submenu arrow on the right; `add_contents` fills the submenu.
pub fn submenu<R>(
    ui: &mut Ui,
    title: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let atoms = (check_column(ui, false), title.to_owned());
    ui.menu_button(atoms, add_contents)
}
