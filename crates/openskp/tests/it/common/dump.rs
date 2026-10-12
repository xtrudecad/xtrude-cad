// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The canonical JSON dump of an [`SkpModel`], in the shape of the
//! fixtures' `*.expected.json` reference dumps, and the comparison the
//! golden check runs on two dumps.

use openskp::{AttrValue, Definition, Dimension, IndexMap, SkpModel, Texture};
use serde_json::{json, Value};

/// A string-keyed map as `[key, value]` pairs, in insertion order.
trait Pairs<V> {
    fn pairs(&self, f: impl Fn(&V) -> Value) -> Vec<Value>;
}

impl<V> Pairs<V> for IndexMap<String, V> {
    fn pairs(&self, f: impl Fn(&V) -> Value) -> Vec<Value> {
        self.iter().map(|(k, v)| json!([k, f(v)])).collect()
    }
}

fn num(x: f64) -> Value {
    if x.is_nan() {
        json!("nan")
    } else if x.is_infinite() {
        json!(if x > 0.0 { "inf" } else { "-inf" })
    } else {
        json!(x)
    }
}

fn vec(v: Option<&[f64]>) -> Value {
    v.map_or(Value::Null, |v| {
        Value::Array(v.iter().map(|&c| num(c)).collect())
    })
}

fn attr(v: &AttrValue) -> Value {
    match v {
        AttrValue::Null => Value::Null,
        AttrValue::Str(s) => json!({ "str": s }),
        AttrValue::Int(i) => json!({ "int": i }),
        AttrValue::Float(f) => json!({ "float": num(*f) }),
        AttrValue::Triple(t) => json!({ "tuple": vec(Some(t)) }),
        AttrValue::List(l) => json!({ "list": l.iter().map(attr).collect::<Vec<_>>() }),
    }
}

/// FNV-1a 64 of the texture bytes, as the Python dump hashes them.
pub fn fnv1a64(data: &[u8]) -> String {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    format!("{h:016x}")
}

fn texture(t: Option<&Texture>) -> Value {
    let Some(t) = t else {
        return Value::Null;
    };
    let data = t.data.as_ref().map_or(
        Value::Null,
        |d| json!({ "len": d.len(), "fnv": fnv1a64(d) }),
    );
    json!({ "filename": t.filename, "width": num(t.width), "height": num(t.height), "data": data })
}

fn dimension(x: &Dimension) -> Value {
    json!({
        "text": x.text, "hidden": x.hidden,
        "a": vec(x.a.as_ref().map(|p| &p[..])), "b": vec(x.b.as_ref().map(|p| &p[..])),
        "offset": num(x.offset),
        "plane_x": vec(x.plane_x.as_ref().map(|p| &p[..])),
        "normal": vec(x.normal.as_ref().map(|p| &p[..])),
    })
}

fn opt(p: &Option<[f64; 3]>) -> Value {
    vec(p.as_ref().map(|p| &p[..]))
}

pub fn definition(d: &Definition) -> Value {
    json!({
        "id": d.id, "guid": d.guid, "name": d.name,
        "vertices": d.vertices.iter().map(|(k, v)| json!([k, [v.id, num(v.x), num(v.y), num(v.z)]])).collect::<Vec<_>>(),
        "edges": d.edges.iter().map(|(k, e)| json!([k, {
            "id": e.id, "v1_id": e.v1_id, "v2_id": e.v2_id, "soft": e.soft,
            "smooth": e.smooth, "hidden": e.hidden, "layer": e.layer, "curve_id": e.curve_id,
        }])).collect::<Vec<_>>(),
        "faces": d.faces.iter().map(|(k, f)| json!([k, {
            "id": f.id,
            "loops": f.loops.iter().map(|l| l.iter().map(|(e, o)| json!([e, o])).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "normal": opt(&f.normal),
            "material_id": f.material_id, "back_material_id": f.back_material_id,
            "uv_transform": vec(f.uv_transform.as_ref().map(|m| &m[..])),
            "uv_transform_back": vec(f.uv_transform_back.as_ref().map(|m| &m[..])),
            "uv_projected": f.uv_projected, "uv_projected_back": f.uv_projected_back,
            "hidden": f.hidden, "layer": f.layer,
        }])).collect::<Vec<_>>(),
        "instances": d.instances.iter().map(|i| json!({
            "name": i.name, "ref_idx": i.ref_idx, "guid": i.guid,
            "matrix": vec(Some(&i.matrix)), "layer": i.layer,
            "properties": i.properties.pairs(|v| json!(v)),
            "attribute_dictionaries": i.attribute_dictionaries.pairs(|d| Value::Array(d.pairs(attr))),
            "material_id": i.material_id, "hidden": i.hidden,
        })).collect::<Vec<_>>(),
        "section_planes": d.section_planes.iter().map(|s| json!({
            "plane": vec(Some(&s.plane)), "name": s.name, "label": s.label, "hidden": s.hidden,
        })).collect::<Vec<_>>(),
        "texts": d.texts.iter().map(|t| json!({
            "text": t.text, "hidden": t.hidden, "point": opt(&t.point), "label_point": opt(&t.label_point),
        })).collect::<Vec<_>>(),
        "dimensions": d.dimensions.iter().map(dimension).collect::<Vec<_>>(),
        "construction_lines": d.construction_lines.iter().map(|c| json!({
            "point": vec(Some(&c.point)), "direction": vec(Some(&c.direction)),
            "start": opt(&c.start), "end": opt(&c.end),
        })).collect::<Vec<_>>(),
        "construction_points": d.construction_points.iter().map(|c| vec(Some(&c.position))).collect::<Vec<_>>(),
        "always_faces_camera": d.always_faces_camera,
        "shadows_face_sun": d.shadows_face_sun,
        "is_image": d.is_image,
    })
}

pub fn dump(model: &SkpModel) -> Value {
    json!({
        "version": model.version,
        "units": model.units,
        "definitions": model.definitions.iter().map(|(k, d)| json!([k, definition(d)])).collect::<Vec<_>>(),
        "root": definition(&model.root),
        "layers": model.layers.iter().map(|l| json!({
            "name": l.name, "color": [l.color_r, l.color_g, l.color_b], "hidden": l.hidden,
        })).collect::<Vec<_>>(),
        "pages": model.pages.iter().map(|p| json!({
            "name": p.name, "eye": opt(&p.eye), "target": opt(&p.target), "up": opt(&p.up),
            "fov": num(p.fov), "parallel": p.parallel, "ortho_height": num(p.ortho_height),
            "hidden_layers": p.hidden_layers,
        })).collect::<Vec<_>>(),
        "dimensions": model.dimensions.iter().map(dimension).collect::<Vec<_>>(),
        "materials": model.materials.iter().map(|m| json!({
            "name": m.name, "color": m.color, "transparency": num(m.transparency), "id": m.id,
            "texture": texture(m.texture.as_ref()), "colorized": m.colorized,
            "colorize_type": m.colorize_type,
        })).collect::<Vec<_>>(),
        "materials_by_id": model.materials_by_id.iter().map(|(k, i)| json!([k, i])).collect::<Vec<_>>(),
        "styles": model.styles.iter().map(|s| json!({
            "name": s.name, "front_color": s.front_color, "back_color": s.back_color,
        })).collect::<Vec<_>>(),
    })
}

/// The first place two dumps differ (`$.definitions[3][1].faces…`), or
/// `None` when they agree within `tol` (relative to the magnitude, and at
/// least absolute `tol`).
pub fn first_difference(path: &str, want: &Value, got: &Value, tol: f64) -> Option<String> {
    match (want, got) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_f64() || b.is_f64() {
                let (x, y) = (
                    a.as_f64().unwrap_or(f64::NAN),
                    b.as_f64().unwrap_or(f64::NAN),
                );
                ((x - y).abs() > tol * x.abs().max(1.0))
                    .then(|| format!("{path}: expected {x}, got {y}"))
            } else {
                (a != b).then(|| format!("{path}: expected {a}, got {b}"))
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!(
                    "{path}: expected {} items, got {}",
                    a.len(),
                    b.len()
                ));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| first_difference(&format!("{path}[{i}]"), x, y, tol))
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|k| match (a.get(k), b.get(k)) {
                (Some(x), Some(y)) => first_difference(&format!("{path}.{k}"), x, y, tol),
                (x, y) => Some(format!("{path}.{k}: expected {x:?}, got {y:?}")),
            })
        }
        _ => (want != got).then(|| format!("{path}: expected {want}, got {got}")),
    }
}
