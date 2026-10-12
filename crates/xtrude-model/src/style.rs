// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Display styles — how the model DRAWS (face mode, edge look,
//! background); fed to the viewport live and remembered by saved views.
//!
//! Face modes: `shaded`, `hidden_line`, `monochrome`, `wireframe`, `xray`.

use crate::settings::{self, keys, JsonText};
use serde_json::{json, Value};

use crate::materials::rgb_from_json;

pub const FACE_MODES: [&str; 5] = ["shaded", "hidden_line", "monochrome", "wireframe", "xray"];

/// The classic default back-face blue-grey.
pub const DEFAULT_BACK_COLOR: [f64; 3] = [0.62, 0.70, 0.78];

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub name: String,
    pub face_mode: String,
    pub edges: bool,
    pub profiles: bool,
    pub back_edges: bool,
    pub edge_color: [f64; 3],
    pub front_color: [f64; 3],
    pub back_color: Option<[f64; 3]>,
    pub background: [f64; 3],
    pub sky: bool,
    pub sky_color: [f64; 3],
    pub ground_color: [f64; 3],
    pub section_fill: bool,
    pub section_fill_color: [f64; 3],
}

impl Default for Style {
    fn default() -> Self {
        Style {
            name: "Default".into(),
            face_mode: "shaded".into(),
            edges: true,
            profiles: true,
            back_edges: false,
            edge_color: [0.13, 0.17, 0.23],
            front_color: [1.0, 1.0, 1.0],
            back_color: None,
            background: [0.90, 0.91, 0.92],
            sky: true,
            sky_color: [0.925, 0.935, 0.945],
            ground_color: [0.815, 0.820, 0.815],
            section_fill: true,
            section_fill_color: [0.35, 0.37, 0.41],
        }
    }
}

impl Style {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "face_mode": self.face_mode,
            "edges": self.edges,
            "profiles": self.profiles,
            "back_edges": self.back_edges,
            "edge_color": self.edge_color,
            "front_color": self.front_color,
            "back_color": self.back_color,
            "background": self.background,
            "sky": self.sky,
            "sky_color": self.sky_color,
            "ground_color": self.ground_color,
            "section_fill": self.section_fill,
            "section_fill_color": self.section_fill_color,
        })
    }

    pub fn from_json(raw: &Value) -> Style {
        let d = Style::default();
        let mode = raw
            .get("face_mode")
            .and_then(Value::as_str)
            .unwrap_or("shaded");
        let mode = if FACE_MODES.contains(&mode) {
            mode
        } else {
            "shaded"
        };
        let b = |k: &str, dv: bool| raw.get(k).and_then(Value::as_bool).unwrap_or(dv);
        let c = |k: &str, dv: [f64; 3]| raw.get(k).and_then(rgb_from_json).unwrap_or(dv);
        Style {
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&d.name)
                .to_string(),
            face_mode: mode.to_string(),
            edges: b("edges", d.edges),
            profiles: b("profiles", d.profiles),
            back_edges: b("back_edges", d.back_edges),
            edge_color: c("edge_color", d.edge_color),
            front_color: c("front_color", d.front_color),
            back_color: raw.get("back_color").and_then(rgb_from_json),
            background: c("background", d.background),
            sky: b("sky", d.sky),
            sky_color: c("sky_color", d.sky_color),
            ground_color: c("ground_color", d.ground_color),
            section_fill: b("section_fill", d.section_fill),
            section_fill_color: c("section_fill_color", d.section_fill_color),
        }
    }

    pub fn named(name: &str) -> Style {
        Style {
            name: name.into(),
            ..Style::default()
        }
    }
}

/// The back-face tint that actually draws: the style's own, else the
/// document's adopted tint, else the classic default.
pub fn effective_back_color(style: &Style, adopted: Option<[f64; 3]>) -> [f64; 3] {
    style.back_color.or(adopted).unwrap_or(DEFAULT_BACK_COLOR)
}

pub fn builtin_styles() -> Vec<Style> {
    let white = [1.0, 1.0, 1.0];
    vec![
        Style::named("Default"),
        Style {
            name: "Hidden line".into(),
            face_mode: "hidden_line".into(),
            background: white,
            sky: false,
            ..Style::default()
        },
        Style {
            name: "Monochrome".into(),
            face_mode: "monochrome".into(),
            background: white,
            sky: false,
            ..Style::default()
        },
        Style {
            name: "Wireframe".into(),
            face_mode: "wireframe".into(),
            background: white,
            sky: false,
            ..Style::default()
        },
        Style {
            name: "X-ray".into(),
            face_mode: "xray".into(),
            ..Style::default()
        },
    ]
}

pub fn builtin_names() -> Vec<String> {
    builtin_styles().into_iter().map(|s| s.name).collect()
}

/// A COPY of the style called `name` — a built-in preset first, then the
/// user's saved library.
pub fn style_by_name(name: &str) -> Option<Style> {
    builtin_styles()
        .into_iter()
        .find(|s| s.name == name)
        .or_else(|| user_styles().into_iter().find(|s| s.name == name))
}

/// The saved user styles, in saved order. A broken entry is dropped.
pub fn user_styles() -> Vec<Style> {
    let Some(JsonText(Value::Array(entries))) = settings::stored(&keys::USER_STYLES) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|e| {
            e.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| !n.is_empty())
        })
        .map(Style::from_json)
        .collect()
}

fn store_user_styles(styles: &[Style]) {
    let arr: Vec<Value> = styles.iter().map(Style::to_json).collect();
    settings::write(&keys::USER_STYLES, &JsonText(Value::Array(arr)));
    settings::sync();
}

#[derive(Debug, thiserror::Error)]
#[error("'{0}' is a built-in style")]
pub struct BuiltinStyleName(pub String);

/// Add `style` to the library, replacing a same-named entry; a built-in
/// preset's name is refused.
pub fn save_user_style(style: &Style) -> Result<(), BuiltinStyleName> {
    if builtin_names().contains(&style.name) {
        return Err(BuiltinStyleName(style.name.clone()));
    }
    let mut kept: Vec<Style> = user_styles()
        .into_iter()
        .filter(|s| s.name != style.name)
        .collect();
    kept.push(style.clone());
    store_user_styles(&kept);
    Ok(())
}

pub fn delete_user_style(name: &str) -> bool {
    let styles = user_styles();
    let kept: Vec<Style> = styles.iter().filter(|s| s.name != name).cloned().collect();
    if kept.len() == styles.len() {
        return false;
    }
    store_user_styles(&kept);
    true
}
