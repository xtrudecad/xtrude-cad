// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Section planes — slicing a model to peer inside.
//!
//! An origin + a unit normal. The ACTIVE plane cuts the display —
//! everything on the normal's side disappears. Only ONE plane is the active
//! cut in the model context. Display-only scaffolding, never topology.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{json, Value};
use xtrude_core::math::{DVec3, VecExt};

use crate::guide::{vec3_from_json, vec3_json};
use crate::ids::{next_id, ObjId};

static LAST_UID: AtomicI64 = AtomicI64::new(0);

fn take_uid(wanted: Option<i64>) -> i64 {
    match wanted {
        None => LAST_UID.fetch_add(1, Ordering::SeqCst) + 1,
        Some(w) => {
            LAST_UID.fetch_max(w, Ordering::SeqCst);
            w
        }
    }
}

/// The letter the next section plane takes: A, B, C… then AA, AB.
pub fn next_symbol(planes: &[SectionPlane]) -> String {
    let taken: std::collections::HashSet<String> = planes
        .iter()
        .map(|p| p.symbol.trim().to_uppercase())
        .collect();
    let mut i: i64 = 0;
    loop {
        let mut n = i;
        let mut label = String::new();
        loop {
            label.insert(0, (b'A' + (n % 26) as u8) as char);
            n = n / 26 - 1;
            if n < 0 {
                break;
            }
        }
        if !taken.contains(&label) {
            return label;
        }
        i += 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionPlane {
    pub id: ObjId,
    pub point: DVec3,
    pub normal: DVec3,
    pub name: String,
    pub symbol: String,
    pub active: bool,
    /// Stable identity for saved views (.igz SavedView references).
    pub uid: i64,
}

impl SectionPlane {
    pub fn new(
        point: DVec3,
        normal: DVec3,
        name: &str,
        symbol: &str,
        active: bool,
        uid: Option<i64>,
    ) -> Self {
        let n = if normal.length() < 1e-12 {
            DVec3::Z
        } else {
            normal
        };
        SectionPlane {
            id: next_id(),
            point,
            normal: n.unit_or_zero(),
            name: name.to_string(),
            symbol: symbol.to_string(),
            active,
            uid: take_uid(uid),
        }
    }

    /// Reverse: the cut hides the other side.
    pub fn flip(&mut self) {
        self.normal = -self.normal;
    }

    /// Signed distance: > 0 is the HIDDEN side.
    pub fn side(&self, p: DVec3) -> f64 {
        self.normal.dot(p - self.point)
    }

    pub fn to_json(&self) -> Value {
        let mut e = json!({
            "point": vec3_json(self.point),
            "normal": vec3_json(self.normal),
            "uid": self.uid,
        });
        if !self.name.is_empty() {
            e["name"] = json!(self.name);
        }
        if !self.symbol.is_empty() {
            e["symbol"] = json!(self.symbol);
        }
        if self.active {
            e["active"] = json!(true);
        }
        e
    }

    pub fn from_json(raw: &Value) -> Option<SectionPlane> {
        Some(SectionPlane::new(
            vec3_from_json(raw.get("point")?)?,
            vec3_from_json(raw.get("normal")?)?,
            raw.get("name").and_then(Value::as_str).unwrap_or(""),
            raw.get("symbol").and_then(Value::as_str).unwrap_or(""),
            raw.get("active").and_then(Value::as_bool).unwrap_or(false),
            raw.get("uid").and_then(Value::as_i64),
        ))
    }
}
