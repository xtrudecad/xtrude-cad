// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Preferences dialog opened and closed (drawn by [`super::prefs_dialog`]),
//! the About dialog, and the theme as egui visuals.

use xtrude_model::i18n::tr;

use super::prefs_dialog::{preferences_modal, PrefsState};
use super::XtrudeApp;
use crate::theme::{self, Palette};
use crate::window::preferences::Preferences;

fn c32(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// The palette as egui visuals (Fusion-like: flat panels, the accent for
/// selection).
pub fn visuals(dark: bool, p: &Palette) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = Some(c32(p.window_text));
    v.panel_fill = c32(p.window);
    v.window_fill = c32(p.window);
    v.extreme_bg_color = c32(p.base);
    v.faint_bg_color = c32(p.alternate_base);
    v.code_bg_color = c32(p.base);
    v.hyperlink_color = c32(p.link);
    v.error_fg_color = c32(p.bright_text);
    v.selection.bg_fill = c32(p.highlight);
    v.selection.stroke.color = c32(p.highlighted_text);
    v.widgets.noninteractive.bg_fill = c32(p.window);
    v.widgets.noninteractive.bg_stroke.color = c32(p.mid);
    v.widgets.inactive.bg_fill = c32(p.light);
    v.widgets.inactive.weak_bg_fill = c32(p.light);
    v.widgets.hovered.weak_bg_fill = c32(p.midlight);
    v.widgets.active.weak_bg_fill = c32(p.mid);
    v.window_stroke.color = c32(p.shadow);
    v
}

impl XtrudeApp {
    /// The saved theme, re-resolved against the desktop every frame (the
    /// `system` mode follows a live flip).
    pub(super) fn apply_theme(&mut self, ctx: &egui::Context) {
        let mode = theme::saved_theme();
        let system = ctx.system_theme().map(|t| t == egui::Theme::Dark);
        let dark = theme::is_dark(&mode, system);
        if self.theme_dark != Some(dark) {
            self.theme_dark = Some(dark);
            let p = theme::palette_for(Some(&mode), system);
            ctx.set_visuals(visuals(dark, &p));
            self.palette = p;
        }
    }

    pub(super) fn open_preferences(&mut self) {
        self.prefs = Some(PrefsState::new(Preferences::load(&self.window)));
    }

    pub(super) fn open_about(&mut self) {
        self.about = true;
    }

    pub(super) fn preferences_dialog(&mut self, ctx: &egui::Context) {
        let Some(state) = self.prefs.as_mut() else {
            return;
        };
        let Some(ok) = preferences_modal(ctx, state).close else {
            return;
        };
        if let Some(state) = self.prefs.take() {
            if ok {
                state.prefs.accept(&mut self.window);
                self.theme_dark = None;
            }
        }
    }

    pub(super) fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let logo = self.about_logo(ctx);
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
            ui.set_width(ABOUT_WIDTH);
            ui.vertical_centered(|ui| {
                ui.add_space(ABOUT_GAP);
                match logo {
                    Some(tex) => {
                        let size = tex.size_vec2();
                        let w = ABOUT_LOGO_WIDTH.min(ui.available_width());
                        ui.image((tex.id(), egui::vec2(w, w * size.y / size.x)));
                    }
                    None => {
                        ui.label(egui::RichText::new("Xtrude CAD").strong().size(22.0));
                    }
                }
                ui.add_space(ABOUT_GAP);
                ui.label(
                    egui::RichText::new(tr("Technical 3D modeling for 3D printing.", &[]))
                        .size(15.0),
                );
                ui.add_space(ABOUT_GAP);
                ui.label(format!("{} {}", tr("Version", &[]), xtrude_model::VERSION));
                if let Some(gpu) = self.window.gpu_line() {
                    ui.add_space(ABOUT_GAP / 2.0);
                    ui.small(gpu);
                }
                ui.add_space(ABOUT_GAP);
                ui.small(tr("Licensed under GPL-3.0-or-later", &[]));
                ui.add_space(ABOUT_GAP * 1.5);
                if ui
                    .add(egui::Button::new(tr("OK", &[])).min_size(egui::vec2(96.0, 0.0)))
                    .clicked()
                {
                    close = true;
                }
                ui.add_space(ABOUT_GAP / 2.0);
            });
        });
        if close || modal.should_close() {
            self.about = false;
        }
    }

    /// The lockup for the current theme as a texture, made once per theme.
    fn about_logo(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let dark = self.theme_dark.unwrap_or(false);
        if let Some((d, tex)) = &self.about_logo {
            if *d == dark {
                return Some(tex.clone());
            }
        }
        let img = lockup_image(dark)?;
        let tex = ctx.load_texture("about:lockup", img, egui::TextureOptions::LINEAR);
        self.about_logo = Some((dark, tex.clone()));
        Some(tex)
    }
}

/// The About box's width in points.
const ABOUT_WIDTH: f32 = 560.0;
/// The space between the About box's lines, in points.
const ABOUT_GAP: f32 = 18.0;
/// The lockup's width in the About box, in points.
const ABOUT_LOGO_WIDTH: f32 = 400.0;

/// The icon-and-wordmark lockup for a dark or a light background.
fn lockup_image(dark: bool) -> Option<egui::ColorImage> {
    let bytes: &[u8] = if dark {
        include_bytes!("../../../../resources/xtrude-dark.png")
    } else {
        include_bytes!("../../../../resources/xtrude-light.png")
    };
    let pixmap = tiny_skia::Pixmap::decode_png(bytes).ok()?;
    Some(egui::ColorImage::from_rgba_premultiplied(
        [pixmap.width() as usize, pixmap.height() as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::lockup_image;

    #[test]
    fn both_lockups_decode() {
        for dark in [false, true] {
            let img = lockup_image(dark).expect("the lockup decodes");
            assert_eq!(img.size, [1200, 226]);
        }
    }
}
