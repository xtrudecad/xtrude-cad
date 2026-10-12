// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Styles: pick a style (built-ins and the user library),
//! edit the active one live, save it under a name, delete a saved one.

use xtrude_model::i18n::tr;
use xtrude_model::style::{
    builtin_styles, delete_user_style, effective_back_color, save_user_style, style_by_name,
    user_styles,
};

use crate::window::MainWindow;

/// `(mode key, label)`.
pub const MODES: [(&str, &str); 5] = [
    ("shaded", "Shaded"),
    ("hidden_line", "Hidden line"),
    ("monochrome", "Monochrome"),
    ("wireframe", "Wireframe"),
    ("xray", "X-ray"),
];

/// The colour swatches: `(attr, tooltip)`.
pub const SWATCHES: [(&str, &str); 7] = [
    ("edge_color", "Edge color"),
    (
        "front_color",
        "Front color — paints faces in Hidden line and Monochrome",
    ),
    ("back_color", "Back color — paints faces seen from behind"),
    ("sky_color", "Sky color"),
    ("ground_color", "Ground color"),
    ("background", "Background — visible with the sky off"),
    ("section_fill_color", "Section fill color"),
];

#[derive(Default)]
pub struct Styles {
    /// `(label, style name)`; `None` = the separator before saved styles.
    pub combo: Vec<Option<(String, String)>>,
    /// The entry showing the active style (none when it matches none).
    pub current: Option<usize>,
    pub delete_enabled: bool,
}

impl Styles {
    pub fn refresh(&mut self, w: &MainWindow) {
        let style = &w.viewport.scene.display_style;
        let users = user_styles();
        self.combo = builtin_styles()
            .into_iter()
            .map(|s| Some((tr(&s.name, &[]), s.name)))
            .collect();
        if !users.is_empty() {
            self.combo.push(None);
            self.combo
                .extend(users.iter().map(|s| Some((s.name.clone(), s.name.clone()))));
        }
        self.current = self
            .combo
            .iter()
            .position(|e| e.as_ref().is_some_and(|(_, n)| *n == style.name));
        self.delete_enabled = users.iter().any(|s| s.name == style.name);
    }

    /// The index of the entry named `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.combo
            .iter()
            .position(|e| e.as_ref().is_some_and(|(_, n)| n == name))
    }

    /// The combo's pick: the style becomes the scene's.
    pub fn pick(&mut self, w: &mut MainWindow, index: usize) {
        let Some(Some((_, name))) = self.combo.get(index).cloned() else {
            return;
        };
        let Some(picked) = style_by_name(&name) else {
            return;
        };
        w.viewport.scene.display_style = picked;
        w.sync_style_menu();
        w.viewport.update();
        self.refresh(w);
    }

    /// An edit of the controls: the active style changes live.
    pub fn edit(&mut self, w: &mut MainWindow, f: impl FnOnce(&mut xtrude_model::style::Style)) {
        f(&mut w.viewport.scene.display_style);
        w.sync_style_menu();
        w.viewport.update();
        self.refresh(w);
    }

    /// The colour a swatch shows (Back: the tint that actually draws).
    pub fn shown_color(w: &MainWindow, attr: &str) -> [f64; 3] {
        let s = &w.viewport.scene.display_style;
        match attr {
            "edge_color" => s.edge_color,
            "front_color" => s.front_color,
            "back_color" => effective_back_color(s, w.viewport.scene.back_face_color),
            "sky_color" => s.sky_color,
            "ground_color" => s.ground_color,
            "background" => s.background,
            _ => s.section_fill_color,
        }
    }

    /// A swatch clicked: pick its colour; say why when it cannot show now.
    pub fn pick_color(&mut self, w: &mut MainWindow, title: &str, attr: &str) {
        let current = Self::shown_color(w, attr);
        let Some(c) = w.dialogs.get_color(title, current) else {
            return;
        };
        let s = &mut w.viewport.scene.display_style;
        match attr {
            "edge_color" => s.edge_color = c,
            "front_color" => s.front_color = c,
            "back_color" => s.back_color = Some(c),
            "sky_color" => s.sky_color = c,
            "ground_color" => s.ground_color = c,
            "background" => s.background = c,
            _ => s.section_fill_color = c,
        }
        self.refresh(w);
        w.viewport.update();
        if let Some(h) = self.color_hint(w, attr) {
            w.show_message(&h, 4000);
        }
    }

    /// Why an edited colour may not show right now.
    pub fn color_hint(&self, w: &MainWindow, attr: &str) -> Option<String> {
        let s = &w.viewport.scene.display_style;
        let mode = s.face_mode.as_str();
        let t = |m: &str| Some(tr(m, &[]));
        match attr {
            "front_color" if mode != "hidden_line" && mode != "monochrome" => {
                t("Front color saved — it paints faces in Hidden line and Monochrome modes.")
            }
            "back_color" if mode == "hidden_line" || mode == "wireframe" => t(
                "Back color saved — it paints back faces in Shaded, Monochrome and X-ray modes.",
            ),
            "background" if s.sky => t("Background saved — it shows with the sky off."),
            "sky_color" | "ground_color" if !s.sky => {
                t("Sky and ground colors show with the sky on.")
            }
            "edge_color" if !s.edges && mode != "wireframe" => {
                t("Edge color saved — this style has edges off.")
            }
            "section_fill_color" if !s.section_fill => {
                t("Section fill color saved — section fill is off in this style.")
            }
            _ => None,
        }
    }

    /// «Save style…»: the current look under a name in the user library.
    pub fn save(&mut self, w: &mut MainWindow) {
        let builtins: Vec<String> = builtin_styles().into_iter().map(|s| s.name).collect();
        let current = w.viewport.scene.display_style.name.clone();
        let suggested = if builtins.contains(&current) {
            String::new()
        } else {
            current
        };
        let Some(name) = w.dialogs.get_text(
            &tr("Save style", &[]),
            &tr("Style name:", &[]),
            &suggested,
            false,
        ) else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if builtins.contains(&name) {
            w.dialogs.warn(
                &tr("Save style", &[]),
                &tr(
                    "'{name}' is a built-in style — pick another name.",
                    &[("name", &name)],
                ),
            );
            return;
        }
        w.viewport.scene.display_style.name = name.clone();
        let _ = save_user_style(&w.viewport.scene.display_style);
        w.sync_style_menu();
        self.refresh(w);
        w.show_message(&tr("Style '{name}' saved.", &[("name", &name)]), 3000);
    }

    /// «Delete»: the saved style shown in the combo.
    pub fn delete(&mut self, w: &mut MainWindow) {
        let Some(Some((_, name))) = self.current.and_then(|i| self.combo.get(i).cloned()) else {
            return;
        };
        if !w.dialogs.confirm(
            &tr("Delete style", &[]),
            &tr("Delete style '{name}'?", &[("name", &name)]),
        ) {
            return;
        }
        delete_user_style(&name);
        w.sync_style_menu();
        self.refresh(w);
    }
}
