// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Model Info dialog on screen: two tabs of read-only tables, and Refresh,
//! Copy to clipboard and Close. The numbers come from `window::model_info`.

use xtrude_model::i18n::tr;

use super::XtrudeApp;
use crate::window::model_info::{
    collect_stats, color_rows, geometry_rows, stats_to_text, ModelStats, COPIED_MS, NAME,
};

/// The open Model Info dialog.
pub struct ModelInfoState {
    stats: ModelStats,
    tab: usize,
    /// When the title stops saying "copied!" (egui time, seconds).
    copied_until: Option<f64>,
}

fn rgb32(c: [f64; 3]) -> egui::Color32 {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    egui::Color32::from_rgb(r, g, b)
}

/// A read-only table with a header row and alternating row colours.
fn table(ui: &mut egui::Ui, id: &str, headers: &[String], body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .striped(true)
        .num_columns(headers.len())
        .min_col_width(60.0)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for h in headers {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            body(ui);
        });
}

impl XtrudeApp {
    pub(super) fn open_model_info(&mut self) {
        self.model_info = Some(ModelInfoState {
            stats: collect_stats(&self.window.viewport.scene),
            tab: 0,
            copied_until: None,
        });
    }

    pub(super) fn model_info_dialog(&mut self, ctx: &egui::Context) {
        let scene = &self.window.viewport.scene;
        let Some(state) = self.model_info.as_mut() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if state.copied_until.is_some_and(|t| now >= t) {
            state.copied_until = None;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("model_info")).show(ctx, |ui| {
            ui.set_width(520.0);
            let mut title = tr(NAME, &[]);
            if state.copied_until.is_some() {
                title = format!("{title} — {}", tr("copied!", &[]));
            }
            ui.heading(title);
            ui.horizontal(|ui| {
                for (i, t) in ["Geometry", "Colors & Layers"].iter().enumerate() {
                    ui.selectable_value(&mut state.tab, i, tr(t, &[]));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .min_scrolled_height(460.0)
                .max_height(460.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if state.tab == 0 {
                        let headers = [tr("Property", &[]), tr("Value", &[])];
                        table(ui, "model_info_geom", &headers, |ui| {
                            for (prop, val) in geometry_rows(&state.stats) {
                                if val.is_empty() {
                                    // section header row
                                    ui.label(egui::RichText::new(prop).strong());
                                } else {
                                    ui.label(prop);
                                }
                                ui.label(val);
                                ui.end_row();
                            }
                        });
                    } else {
                        ui.label(tr("Colors in use", &[]));
                        let headers = [tr("Color", &[]), tr("Faces", &[]), tr("Area", &[])];
                        table(ui, "model_info_mats", &headers, |ui| {
                            for row in color_rows(&state.stats) {
                                match row.swatch {
                                    Some(rgb) => {
                                        let text = if row.dark_text {
                                            egui::Color32::BLACK
                                        } else {
                                            egui::Color32::WHITE
                                        };
                                        egui::Frame::NONE
                                            .fill(rgb32(rgb))
                                            .inner_margin(egui::Margin::symmetric(4, 1))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    egui::RichText::new(&row.label).color(text),
                                                );
                                            });
                                    }
                                    None => {
                                        ui.label(&row.label);
                                    }
                                }
                                ui.label(&row.faces);
                                ui.label(&row.area);
                                ui.end_row();
                            }
                        });
                        ui.add_space(8.0);
                        ui.label(tr("Layers", &[]));
                        let headers = [tr("Layer", &[]), tr("Visible", &[]), tr("Locked", &[])];
                        table(ui, "model_info_layers", &headers, |ui| {
                            // ✓/✗ are not in egui's fonts; its emoji font
                            // has these
                            for ly in &state.stats.layers {
                                ui.label(&ly.name);
                                ui.label(if ly.visible { "✔" } else { "✖" });
                                ui.label(if ly.locked { "🔒" } else { "—" });
                                ui.end_row();
                            }
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr("Refresh", &[])).clicked() {
                    state.stats = collect_stats(scene);
                }
                if ui.button(tr("Copy to clipboard", &[])).clicked() {
                    ui.ctx().copy_text(stats_to_text(&state.stats));
                    state.copied_until = Some(now + COPIED_MS as f64 / 1000.0);
                }
                if ui.button(tr("Close", &[])).clicked() {
                    close = true;
                }
            });
        });
        if let Some(t) = state.copied_until {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((t - now).max(0.0)));
        }
        if close || modal.should_close() {
            self.model_info = None;
        }
    }
}
