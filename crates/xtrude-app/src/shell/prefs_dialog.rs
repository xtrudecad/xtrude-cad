// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Preferences dialog on screen: the page list on the left, the chosen
//! page's titled groups on the right, OK and Cancel under them. The dialog
//! keeps one size whatever the page, so switching pages neither moves nor
//! resizes it; on a window too small for it, it shrinks to fit and the page
//! scrolls.

use egui::{vec2, Rect, Vec2};
use xtrude_model::i18n::tr;
use xtrude_model::units::{unit_label, UNIT_CHOICES};

use super::question_frame;
use crate::viewport::NavStyle;
use crate::window::preferences::{
    Preferences, PrefsField, PrefsGroup, PrefsPage, DXF_UNITS, MSAA_CHOICES, OBJ_UNITS,
    PLATFORM_CHOICES, REST_MODES, THEME_CHOICES, TOOLBAR_ICON_SIZES,
};

/// The dialog's size inside its frame's margin, when the window has room.
const DIALOG_SIZE: Vec2 = vec2(820.0, 540.0);
/// The least room kept between the dialog's frame and the window's edges.
const SCREEN_GAP: f32 = 16.0;
/// The page list's width.
const SIDEBAR_WIDTH: f32 = 160.0;
/// The margin inside the page list's frame.
const SIDEBAR_MARGIN: i8 = 6;
/// The height of an entry of the page list.
const ENTRY_HEIGHT: f32 = 30.0;
/// The space between the page list and the page.
const COLUMN_GAP: f32 = 16.0;
/// The page heading's text size.
const PAGE_HEADING_SIZE: f32 = 20.0;
/// The space under the page heading, and between two groups.
const GROUP_GAP: f32 = 16.0;
/// The space between a group's title and its frame.
const GROUP_TITLE_GAP: f32 = 4.0;
/// The margin inside a group's frame.
const GROUP_MARGIN: i8 = 12;
/// The space between two rows of a group, and between its two columns.
const ROW_SPACING: Vec2 = vec2(16.0, 10.0);
/// The narrowest a group's label column is, so the groups of a page line up.
const LABEL_WIDTH: f32 = 190.0;
/// A drop-down's width.
const COMBO_WIDTH: f32 = 260.0;

/// The open Preferences dialog: its fields and page.
pub struct PrefsState {
    pub prefs: Preferences,
    pub page: PrefsPage,
}

impl PrefsState {
    /// The dialog as it opens: on the General page.
    pub fn new(prefs: Preferences) -> PrefsState {
        PrefsState {
            prefs,
            page: PrefsPage::General,
        }
    }
}

/// What one frame of the dialog did, and where it drew.
pub struct PrefsFrame {
    /// `Some(true)` for OK, `Some(false)` for Cancel or Escape.
    pub close: Option<bool>,
    /// The whole dialog, its frame included.
    pub dialog: Rect,
    /// The page list's entries, in [`PrefsPage::ALL`]'s order.
    pub entries: Vec<Rect>,
    /// The page area: the page heading and its groups.
    pub page_area: Rect,
    /// The OK button.
    pub ok: Rect,
}

/// Shows the dialog for one frame.
pub fn preferences_modal(ctx: &egui::Context, state: &mut PrefsState) -> PrefsFrame {
    let margin = question_frame::MARGIN;
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(margin);
    let room = ctx.content_rect().size() - Vec2::splat(2.0 * (f32::from(margin) + SCREEN_GAP));
    let size = DIALOG_SIZE.min(room).max(Vec2::ZERO);
    let mut entries = Vec::new();
    let mut page_area = Rect::NOTHING;
    let mut ok = Rect::NOTHING;
    let mut close = None;
    let modal = egui::Modal::new(egui::Id::new("preferences"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.heading(format!("{} — Xtrude CAD", tr("Preferences", &[])));
            ui.add_space(question_frame::HEADING_GAP);
            let body_height = (ui.available_height()
                - question_frame::button_row_height()
                - ui.spacing().item_spacing.y)
                .max(0.0);
            let body = vec2(size.x, body_height);
            ui.allocate_ui_with_layout(body, egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_min_size(body);
                entries = page_list(ui, &mut state.page, body_height);
                ui.add_space(COLUMN_GAP);
                let page = vec2(ui.available_width(), body_height);
                page_area = ui
                    .allocate_ui_with_layout(page, egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_min_size(page);
                        page_content(ui, state);
                    })
                    .response
                    .rect;
            });
            question_frame::button_row(ui, |ui| {
                if question_frame::button(ui, tr("Cancel", &[])).clicked() {
                    close = Some(false);
                }
                let r = question_frame::button(ui, tr("OK", &[]));
                if r.clicked() {
                    close = Some(true);
                }
                ok = r.rect;
            });
        });
    if modal.should_close() && close.is_none() {
        close = Some(false);
    }
    PrefsFrame {
        close,
        dialog: modal.response.rect,
        entries,
        page_area,
        ok,
    }
}

/// The page list, full height; returns its entries' rects.
fn page_list(ui: &mut egui::Ui, current: &mut PrefsPage, height: f32) -> Vec<Rect> {
    let mut rects = Vec::new();
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(4)
        .inner_margin(SIDEBAR_MARGIN)
        .show(ui, |ui| {
            let inner = vec2(
                SIDEBAR_WIDTH - 2.0 * f32::from(SIDEBAR_MARGIN),
                height - 2.0 * f32::from(SIDEBAR_MARGIN),
            );
            ui.set_min_size(inner);
            ui.set_max_width(inner.x);
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for page in PrefsPage::ALL {
                    let selected = *current == page;
                    let mut text = egui::RichText::new(tr(page.label(), &[]));
                    if selected {
                        // the theme's text colour overrides egui's selected
                        // text colour, which reads poorly on the highlight
                        text = text.color(ui.visuals().selection.stroke.color);
                    }
                    let entry = egui::Button::selectable(selected, text)
                        .min_size(vec2(inner.x, ENTRY_HEIGHT));
                    let r = ui.add(entry);
                    if r.clicked() {
                        *current = page;
                    }
                    rects.push(r.rect);
                }
            });
        });
    rects
}

/// The current page: its heading, then its groups, scrolling when the
/// dialog is too small for them.
fn page_content(ui: &mut egui::Ui, state: &mut PrefsState) {
    ui.label(
        egui::RichText::new(tr(state.page.label(), &[]))
            .size(PAGE_HEADING_SIZE)
            .strong(),
    );
    ui.add_space(GROUP_GAP - ui.spacing().item_spacing.y);
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        for (i, group) in state.page.groups().iter().enumerate() {
            if i > 0 {
                ui.add_space(GROUP_GAP);
            }
            group_ui(ui, group, &mut state.prefs);
        }
    });
}

/// One titled group: its fields in a two-column grid, then its note.
fn group_ui(ui: &mut egui::Ui, group: &PrefsGroup, p: &mut Preferences) {
    ui.label(egui::RichText::new(tr(group.title, &[])).strong());
    ui.add_space(GROUP_TITLE_GAP - ui.spacing().item_spacing.y);
    egui::Frame::group(ui.style())
        .inner_margin(GROUP_MARGIN)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // keyed by title: a group of another page has its own column widths
            egui::Grid::new(("prefs-group", group.title))
                .num_columns(2)
                .min_col_width(LABEL_WIDTH)
                .spacing(ROW_SPACING)
                .show(ui, |ui| {
                    for f in group.fields {
                        field(ui, *f, p);
                    }
                });
            if let Some(note) = group.note {
                ui.add_space(ROW_SPACING.y);
                ui.add(egui::Label::new(tr(note, &[])).wrap());
            }
        });
}

/// One setting: its label, its control, and the row's end.
fn field(ui: &mut egui::Ui, f: PrefsField, p: &mut Preferences) {
    match f {
        PrefsField::Theme => {
            ui.label(tr("Theme:", &[]));
            combo(ui, "theme", &mut p.theme, &strs(&THEME_CHOICES));
        }
        PrefsField::ToolbarIcons => {
            ui.label(tr("Toolbar icons:", &[]));
            let sizes: Vec<(u32, String)> = TOOLBAR_ICON_SIZES
                .iter()
                .map(|(px, l)| (*px, format!("{} ({px} px)", tr(l, &[]))))
                .collect();
            combo(ui, "icon_px", &mut p.icon_px, &sizes);
        }
        PrefsField::RestOfModel => {
            ui.label(tr("Rest of model while editing:", &[]));
            let modes = REST_MODES.map(|(m, l)| (m, tr(l, &[])));
            combo(ui, "rest", &mut p.rest, &modes);
        }
        PrefsField::AskSectionName => {
            ui.label("");
            ui.checkbox(
                &mut p.ask_section,
                tr(
                    "Ask for a name and symbol when placing a section plane",
                    &[],
                ),
            );
        }
        PrefsField::Backup => {
            ui.label("");
            ui.checkbox(
                &mut p.backup,
                tr("Keep a backup of the previous save (.igz.bak)", &[]),
            );
        }
        PrefsField::UndoSteps => {
            ui.label(tr("Undo steps", &[]));
            ui.add(
                egui::DragValue::new(&mut p.undo_steps)
                    .range(0..=5000)
                    .custom_formatter(|n, _| {
                        if n == 0.0 {
                            tr("unlimited", &[])
                        } else {
                            format!("{n}")
                        }
                    }),
            )
            .on_hover_text(tr("Steps kept for Undo. Each one holds a snapshot of the model, so fewer steps mean less memory on a big model; 0 = unlimited.", &[]));
        }
        PrefsField::NavStyle => {
            ui.label(tr("Mouse navigation:", &[]));
            let styles: Vec<(NavStyle, String)> = NavStyle::ALL
                .iter()
                .map(|s| (*s, tr(s.label(), &[])))
                .collect();
            combo(ui, "nav_style", &mut p.nav_style, &styles);
            ui.end_row();
            ui.label("");
            egui::Grid::new("nav_bindings")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for (what, how) in p.nav_style.cheat_sheet() {
                        ui.weak(what);
                        ui.weak(how);
                        ui.end_row();
                    }
                });
        }
        PrefsField::InvertWheel => {
            ui.label("");
            ui.checkbox(&mut p.invert, tr("Invert mouse wheel zoom", &[]));
        }
        PrefsField::InvertOrbit => {
            ui.label("");
            ui.checkbox(&mut p.invert_orbit, tr("Invert vertical orbit", &[]));
        }
        PrefsField::Msaa => {
            ui.label(tr("Anti-aliasing (MSAA):", &[]));
            let msaa: Vec<(u32, String)> = MSAA_CHOICES
                .iter()
                .map(|n| {
                    let label = if *n == 0 {
                        tr("Off", &[])
                    } else {
                        format!("{n}x")
                    };
                    (*n, label)
                })
                .collect();
            combo(ui, "msaa", &mut p.msaa, &msaa);
        }
        PrefsField::Platform => {
            ui.label(tr("Graphics server:", &[]));
            combo(ui, "platform", &mut p.platform, &strs(&PLATFORM_CHOICES));
        }
        PrefsField::ObjUnit => {
            ui.label(tr("Suggested unit for OBJ:", &[]));
            combo(ui, "obj", &mut p.obj_unit, &strs(&OBJ_UNITS));
        }
        PrefsField::DxfUnit => {
            ui.label(tr("Suggested unit for DXF:", &[]));
            combo(ui, "dxf", &mut p.dxf_unit, &strs(&DXF_UNITS));
        }
        PrefsField::LengthUnit => {
            ui.label(tr("Length unit", &[]));
            let units: Vec<(String, String)> = UNIT_CHOICES
                .iter()
                .map(|c| (c.to_string(), unit_label(c)))
                .collect();
            combo(ui, "unit", &mut p.unit, &units);
        }
        PrefsField::Decimals => {
            ui.label(tr("Decimals", &[]));
            ui.add(egui::DragValue::new(&mut p.decimals).range(0..=6));
        }
        PrefsField::UnitsForNew => {
            ui.label("");
            ui.checkbox(&mut p.units_for_new, tr("Also use for new documents", &[]));
        }
    }
    ui.end_row();
}

fn combo<T: PartialEq + Clone>(ui: &mut egui::Ui, id: &str, value: &mut T, items: &[(T, String)]) {
    let label = items
        .iter()
        .find(|(k, _)| k == value)
        .map(|(_, l)| l.clone())
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(COMBO_WIDTH)
        .show_ui(ui, |ui| {
            for (k, l) in items {
                if ui.selectable_label(k == value, l).clicked() {
                    *value = k.clone();
                }
            }
        });
}

fn strs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(k, l)| (k.to_string(), tr(l, &[])))
        .collect()
}
