// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Leader text — an annotation label pointing at the model: a world
//! `anchor` and an `offset` to where the label floats.

use serde_json::{json, Value};
use xtrude_core::math::DVec3;

use crate::guide::{vec3_from_json, vec3_json};
use crate::ids::{next_id, ObjId};

#[derive(Clone, Debug, PartialEq)]
pub struct TextLabel {
    pub id: ObjId,
    pub anchor: DVec3,
    pub offset: DVec3,
    pub text: String,
    pub layer: Option<String>,
}

impl TextLabel {
    pub fn new(anchor: DVec3, offset: DVec3, text: &str) -> Self {
        TextLabel {
            id: next_id(),
            anchor,
            offset,
            text: text.to_string(),
            layer: None,
        }
    }

    /// Where the label text floats (anchor + offset).
    pub fn position(&self) -> DVec3 {
        self.anchor + self.offset
    }

    pub fn to_json(&self) -> Value {
        let mut e = json!({
            "anchor": vec3_json(self.anchor),
            "offset": vec3_json(self.offset),
            "text": self.text,
        });
        if let Some(l) = &self.layer {
            if !l.is_empty() {
                e["layer"] = json!(l);
            }
        }
        e
    }

    pub fn from_json(raw: &Value) -> TextLabel {
        let mut t = TextLabel::new(
            raw.get("anchor")
                .and_then(vec3_from_json)
                .unwrap_or(DVec3::ZERO),
            raw.get("offset")
                .and_then(vec3_from_json)
                .unwrap_or(DVec3::ZERO),
            raw.get("text").and_then(Value::as_str).unwrap_or(""),
        );
        t.layer = raw
            .get("layer")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        t
    }
}
