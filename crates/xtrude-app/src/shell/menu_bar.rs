// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The menu bar's top-level buttons. egui's `MenuBar` opens a menu on click
//! only; here, while one bar menu is open, hovering another bar entry opens
//! that one in its place, as desktop menu bars do.

use egui::{Popup, Response, Ui};

/// What [`menu_buttons`] drew.
pub struct MenuButtons {
    /// The top-level buttons, one per title.
    pub buttons: Vec<Response>,
    /// The index of the menu open at the end of the frame, if any.
    pub open: Option<usize>,
}

/// Draws one menu button per title inside `ui` (a `MenuBar`'s ui);
/// `items(ui, index)` fills the open menu of the title at `index`.
pub fn menu_buttons(
    ui: &mut Ui,
    titles: &[String],
    mut items: impl FnMut(&mut Ui, usize),
) -> MenuButtons {
    let buttons: Vec<Response> = titles
        .iter()
        .enumerate()
        .map(|(index, title)| {
            ui.menu_button(title.as_str(), |ui| items(ui, index))
                .response
        })
        .collect();
    let popups: Vec<_> = buttons.iter().map(Popup::default_response_id).collect();
    let hovered = buttons.iter().position(Response::hovered);
    let ctx = ui.ctx();
    let open = popups.iter().position(|&id| Popup::is_id_open(ctx, id));
    // Only a hovered entry switches: crossing the gap between two entries
    // or moving down into the open menu hovers none, so the open menu stays.
    match (open, hovered) {
        (Some(open), Some(hovered)) if open != hovered => {
            // opening one popup closes the others; the new menu shows on
            // the next frame
            Popup::open_id(ctx, popups[hovered]);
            ctx.request_repaint();
            MenuButtons {
                buttons,
                open: Some(hovered),
            }
        }
        _ => MenuButtons { buttons, open },
    }
}
