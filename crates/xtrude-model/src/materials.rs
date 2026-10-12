// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The material registry: NAMED materials over the per-face attrs.
//!
//! The ground truth for pixels is the face itself (`attrs.color`,
//! `attrs.opacity`); a [`Material`] gives a name to one recipe and a face
//! painted with it carries `attrs.mat = name` alongside the baked values.

use indexmap::IndexMap;
use serde_json::{json, Value};
use xtrude_core::attrs::{FaceAttrs, Paint, Rgb};

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub color: Option<Rgb>,
    pub opacity: Option<f64>,
}

impl Material {
    pub fn new(name: &str, color: Option<Rgb>, opacity: Option<f64>) -> Self {
        Material {
            name: name.to_string(),
            color,
            opacity,
        }
    }

    /// The paint this material stamps on a face (its own name included).
    pub fn face_paint(&self) -> Paint {
        Paint {
            color: self.color,
            mat: Some(self.name.clone()),
            opacity: self.opacity,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut e = json!({"name": self.name});
        if let Some(c) = self.color {
            e["color"] = json!(c);
        }
        if let Some(o) = self.opacity {
            e["opacity"] = json!(o);
        }
        e
    }

    pub fn from_json(raw: &Value) -> Material {
        Material {
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            color: raw.get("color").and_then(rgb_from_json),
            opacity: raw.get("opacity").and_then(Value::as_f64),
        }
    }
}

/// An RGB triple from a JSON list (first three components).
pub fn rgb_from_json(v: &Value) -> Option<Rgb> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// Whether a face's FRONT material lets light through.
pub fn is_translucent(attrs: &FaceAttrs) -> bool {
    attrs.paint.opacity.unwrap_or(1.0) < 0.999
}

/// Whether the BACK of a face shows the style's default back colour.
pub fn back_is_default(attrs: &FaceAttrs) -> bool {
    if attrs.back.is_some() {
        return false;
    }
    !is_translucent(attrs)
}

/// Add `mat` to the registry, deduplicating. Same name + same recipe →
/// the existing entry wins; same name + different recipe → `"name (2)"`.
pub fn register(materials: &mut IndexMap<String, Material>, mat: Material) -> String {
    let base = if mat.name.is_empty() {
        "Material".to_string()
    } else {
        mat.name.clone()
    };
    let mut name = base.clone();
    let mut n = 2;
    while let Some(other) = materials.get(&name) {
        if other.color == mat.color && other.opacity == mat.opacity {
            return name;
        }
        name = format!("{base} ({n})");
        n += 1;
    }
    let mut m = mat;
    m.name = name.clone();
    materials.insert(name.clone(), m);
    name
}

/// Whether a face was painted itself.
pub fn has_own_material(attrs: &FaceAttrs) -> bool {
    attrs.paint.color.is_some()
}

/// The attrs a face is DRAWN with inside a container that carries
/// `material`: the face's own material wins; only default faces take the
/// container's, front and back alike.
pub fn effective_attrs(attrs: &FaceAttrs, material: Option<&Paint>) -> FaceAttrs {
    let Some(material) = material else {
        return attrs.clone();
    };
    if material.is_empty() || has_own_material(attrs) {
        return attrs.clone();
    }
    let mut merged = attrs.clone();
    if material.color.is_some() {
        merged.paint.color = material.color;
    }
    if material.opacity.is_some() {
        merged.paint.opacity = material.opacity;
    }
    if material.mat.is_some() {
        merged.paint.mat = material.mat.clone();
    }
    if merged.back.is_none() {
        merged.back = Some(xtrude_core::attrs::Back::Mirror);
    }
    merged
}
