// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The layout every question dialog shares: a framed modal with room
//! around its content, a heading, the question's body and a right-aligned
//! row of buttons.

/// The narrowest a dialog is drawn.
pub(super) const DIALOG_MIN_WIDTH: f32 = 440.0;
/// The margin inside the dialog's frame.
pub(super) const MARGIN: i8 = 16;
/// The vertical space between two widgets of the body.
const ITEM_SPACING: f32 = 8.0;
/// The space under the heading.
pub(super) const HEADING_GAP: f32 = 4.0;
/// The space above the button row.
const BUTTONS_GAP: f32 = 12.0;
/// The smallest a button of the row is.
const BUTTON_MIN_SIZE: egui::Vec2 = egui::vec2(88.0, 28.0);

/// Shows the modal `id` titled `title`; `add_contents` draws the body and
/// ends with [`button_row`].
pub(super) fn question_modal<R>(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::ModalResponse<R> {
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(MARGIN);
    egui::Modal::new(id).frame(frame).show(ctx, |ui| {
        ui.set_min_width(DIALOG_MIN_WIDTH);
        ui.spacing_mut().item_spacing.y = ITEM_SPACING;
        ui.heading(title);
        ui.add_space(HEADING_GAP);
        add_contents(ui)
    })
}

/// The dialog's buttons, right-aligned under the body. `add_buttons` adds
/// them from right to left, with [`button`].
pub(super) fn button_row(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(BUTTONS_GAP);
    // as wide as the body: a right-to-left layout would otherwise take
    // every pixel the modal's area allows
    let width = ui.min_rect().width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, BUTTON_MIN_SIZE.y),
        egui::Layout::right_to_left(egui::Align::Center),
        add_buttons,
    );
}

/// The height a [`button_row`] takes, the gap above it included.
pub(super) fn button_row_height() -> f32 {
    BUTTONS_GAP + BUTTON_MIN_SIZE.y
}

/// A button of the [`button_row`].
pub(super) fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(BUTTON_MIN_SIZE))
}
