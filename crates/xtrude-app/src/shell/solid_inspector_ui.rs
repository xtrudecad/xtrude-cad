// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Solid Inspector on screen: a modeless window with the summary, the
//! read-only table, and Refresh, Highlight problems and Close. The rows come
//! from `window::solid_inspector`.

use xtrude_model::i18n::tr;

use super::XtrudeApp;
use crate::window::solid_inspector::{Verdict, NAME};

/// The Status column's colours (`ok` / `bad`).
const OK: egui::Color32 = egui::Color32::from_rgb(80, 160, 80);
const BAD: egui::Color32 = egui::Color32::from_rgb(200, 90, 60);

impl XtrudeApp {
    pub(super) fn solid_inspector_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.window.solid_inspector.as_mut() else {
            return;
        };
        let mut open = true;
        let (mut refresh, mut highlight, mut close) = (false, false, false);
        egui::Window::new(tr(NAME, &[]))
            .id(egui::Id::new("solid_inspector"))
            .open(&mut open)
            .collapsible(false)
            .min_size([560.0, 380.0])
            .default_size([640.0, 440.0])
            .show(ctx, |ui| {
                ui.label(dlg.summary());
                ui.separator();
                let footer = ui.spacing().interact_size.y + 16.0;
                egui::ScrollArea::both()
                    .max_height((ui.available_height() - footer).max(80.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("solid_inspector_rows")
                            .striped(true)
                            .num_columns(5)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                for h in ["Object", "Faces", "Status", "Problems", "Volume"] {
                                    ui.label(egui::RichText::new(tr(h, &[])).strong());
                                }
                                ui.end_row();
                                for i in 0..dlg.rows.len() {
                                    let cells = dlg.cells(i);
                                    let current = dlg.current == Some(i);
                                    // whole-row selection: every cell picks the row
                                    let mut picked = false;
                                    let mut double = false;
                                    for (c, text) in cells.into_iter().enumerate() {
                                        let mut rich = egui::RichText::new(text);
                                        if c == 2 {
                                            match Verdict::of(&dlg.rows[i].report) {
                                                Verdict::Watertight => rich = rich.color(OK),
                                                Verdict::NotWatertight => rich = rich.color(BAD),
                                                Verdict::Empty => {}
                                            }
                                        }
                                        let r = ui.selectable_label(current, rich);
                                        picked |= r.clicked();
                                        double |= r.double_clicked();
                                    }
                                    ui.end_row();
                                    if picked || double {
                                        dlg.current = Some(i);
                                    }
                                    highlight |= double;
                                }
                            });
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    refresh = ui.button(tr("Refresh", &[])).clicked();
                    highlight |= ui.button(tr("Highlight problems", &[])).clicked();
                    close = ui.button(tr("Close", &[])).clicked();
                });
            });
        if refresh {
            dlg.refresh(&self.window.viewport.scene);
        }
        if highlight {
            dlg.highlight(&mut self.window.viewport);
        }
        if close || !open {
            self.window.solid_inspector = None;
        }
    }
}
