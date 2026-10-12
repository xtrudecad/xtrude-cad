// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Layer / tag system: visibility and locking.
//!
//! A layer never *owns* geometry — it is a label an entity carries (faces
//! via `attrs.layer`, edges via their `layer` slot, groups via `.layer`).
//! Hiding a layer removes its entities from render, picking and box
//! selection; locking keeps them visible but unpickable. Everything
//! unlabelled lives on the default layer, which always exists.

use serde_json::{json, Value};

pub const DEFAULT_LAYER: &str = "Layer 0";

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

impl Layer {
    pub fn new(name: &str) -> Self {
        Layer {
            name: name.to_string(),
            visible: true,
            locked: false,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut e = json!({"name": self.name});
        if !self.visible {
            e["visible"] = json!(false);
        }
        if self.locked {
            e["locked"] = json!(true);
        }
        e
    }

    pub fn from_json(raw: &Value) -> Layer {
        Layer {
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_LAYER)
                .to_string(),
            visible: raw.get("visible").and_then(Value::as_bool).unwrap_or(true),
            locked: raw.get("locked").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// The layer name a label resolves to (default when unlabelled).
pub fn layer_name(label: Option<&str>) -> &str {
    match label {
        Some(s) if !s.is_empty() => s,
        _ => DEFAULT_LAYER,
    }
}

/// The stored label for a layer name (`None` for the default layer).
pub fn label_for(name: &str) -> Option<String> {
    if name == DEFAULT_LAYER {
        None
    } else {
        Some(name.to_string())
    }
}
