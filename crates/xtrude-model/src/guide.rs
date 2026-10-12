// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Construction guides (the Measure tool) — guide lines and points.
//!
//! A guide is scaffolding, not geometry: an infinite dashed construction
//! line (or a lone point) used to align real drawing. It never enters the
//! mesh and feeds the snap engine.

use serde_json::{json, Value};
use xtrude_core::math::{DVec3, VecExt};

use crate::ids::{next_id, ObjId};

/// Half-length used when an infinite guide line needs segment form.
pub const GUIDE_HALF_LEN: f64 = 1.0e4;

#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub id: ObjId,
    pub point: DVec3,
    /// `None` = a guide point.
    pub direction: Option<DVec3>,
    /// The vertex a guide point was measured from (draws a guide segment).
    pub origin: Option<DVec3>,
}

impl Guide {
    pub fn new(point: DVec3, direction: Option<DVec3>, origin: Option<DVec3>) -> Self {
        let direction = direction
            .filter(|d| d.length() > 1e-12)
            .map(|d| d.unit_or_zero());
        let origin = if direction.is_none() { origin } else { None };
        Guide {
            id: next_id(),
            point,
            direction,
            origin,
        }
    }

    pub fn is_line(&self) -> bool {
        self.direction.is_some()
    }

    /// The guide as a long finite segment (render clipping and snap).
    pub fn segment(&self) -> (DVec3, DVec3) {
        match self.direction {
            None => (self.point, self.point),
            Some(d) => {
                let d = d * GUIDE_HALF_LEN;
                (self.point - d, self.point + d)
            }
        }
    }

    pub fn a(&self) -> DVec3 {
        self.segment().0
    }

    pub fn b(&self) -> DVec3 {
        self.segment().1
    }

    pub fn to_json(&self) -> Value {
        let mut e = json!({"point": [self.point.x, self.point.y, self.point.z]});
        if let Some(d) = self.direction {
            e["direction"] = json!([d.x, d.y, d.z]);
        }
        if let Some(o) = self.origin {
            e["origin"] = json!([o.x, o.y, o.z]);
        }
        e
    }

    pub fn from_json(raw: &Value) -> Option<Guide> {
        let p = vec3_from_json(raw.get("point")?)?;
        let d = raw.get("direction").and_then(vec3_from_json);
        let o = raw.get("origin").and_then(vec3_from_json);
        Some(Guide::new(p, d, o))
    }
}

/// `[x, y, z]` → DVec3.
pub fn vec3_from_json(v: &Value) -> Option<DVec3> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some(DVec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?))
}

pub fn vec3_json(p: DVec3) -> Value {
    json!([p.x, p.y, p.z])
}
