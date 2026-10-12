// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! glTF 2.0 / GLB export — one self-contained binary file for visual
//! interchange. Painted colours become PBR `baseColorFactor`. glTF is Y-up,
//! right-handed, metres: positions and normals are baked `(x, y, z) →
//! (x, z, −y)` so viewers show the model upright.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};
use xtrude_core::stable_json::round_exact;
use xtrude_core::DVec3;
use xtrude_model::scene::Scene;

use crate::meshexport::{collect_geometry, export_names};

const F32: u32 = 5126;
const U32: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

fn yup(v: DVec3) -> [f64; 3] {
    [v.x, v.z, -v.y]
}

struct Builder {
    buf: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn accessor(
        &mut self,
        data: &[u8],
        comp: u32,
        count: usize,
        ty: &str,
        target: Option<u32>,
        minmax: Option<([f64; 3], [f64; 3])>,
    ) -> usize {
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        let mut view = Map::new();
        view.insert("buffer".into(), json!(0));
        view.insert("byteOffset".into(), json!(self.buf.len()));
        view.insert("byteLength".into(), json!(data.len()));
        if let Some(t) = target {
            view.insert("target".into(), json!(t));
        }
        self.buf.extend_from_slice(data);
        self.views.push(Value::Object(view));
        let mut acc = Map::new();
        acc.insert("bufferView".into(), json!(self.views.len() - 1));
        acc.insert("componentType".into(), json!(comp));
        acc.insert("count".into(), json!(count));
        acc.insert("type".into(), json!(ty));
        if let Some((mn, mx)) = minmax {
            acc.insert("min".into(), json!(mn));
            acc.insert("max".into(), json!(mx));
        }
        self.accessors.push(Value::Object(acc));
        self.accessors.len() - 1
    }
}

fn f32s(vals: &[[f64; 3]]) -> Vec<u8> {
    vals.iter()
        .flat_map(|v| v.iter().flat_map(|&x| (x as f32).to_le_bytes()))
        .collect()
}

/// Write the scene as a binary glTF (`.glb`).
pub fn save_glb(scene: &Scene, path: &Path) -> std::io::Result<()> {
    let (materials, prims) = collect_geometry(scene);
    let names = export_names(&materials);
    let keys: Vec<_> = prims.keys().cloned().collect();
    let mut b = Builder {
        buf: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    let mut gltf_materials = Vec::new();
    let mut mat_index = HashMap::new();
    for key in &keys {
        let info = &materials[key];
        let [r, g, bl] = info.color;
        let mut pbr = Map::new();
        pbr.insert("baseColorFactor".into(), json!([r, g, bl, 1.0]));
        pbr.insert("metallicFactor".into(), json!(0.0));
        pbr.insert("roughnessFactor".into(), json!(1.0));
        let mut mat = Map::new();
        mat.insert("name".into(), json!(names[key]));
        mat.insert("doubleSided".into(), json!(true));
        if let Some(op) = info.opacity {
            pbr.insert("baseColorFactor".into(), json!([r, g, bl, op]));
            mat.insert("pbrMetallicRoughness".into(), Value::Object(pbr));
            mat.insert("alphaMode".into(), json!("BLEND"));
        } else {
            mat.insert("pbrMetallicRoughness".into(), Value::Object(pbr));
        }
        mat_index.insert(key.clone(), gltf_materials.len());
        gltf_materials.push(Value::Object(mat));
    }
    let mut primitives = Vec::new();
    for key in &keys {
        let mut positions: Vec<[f64; 3]> = Vec::new();
        let mut normals: Vec<[f64; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut vindex: HashMap<[u64; 6], u32> = HashMap::new();
        for (n, verts) in &prims[key] {
            let nn = yup(*n);
            for pos in verts {
                let p = yup(*pos);
                let k = [
                    round_exact(p[0], 6).to_bits(),
                    round_exact(p[1], 6).to_bits(),
                    round_exact(p[2], 6).to_bits(),
                    round_exact(nn[0], 4).to_bits(),
                    round_exact(nn[1], 4).to_bits(),
                    round_exact(nn[2], 4).to_bits(),
                ];
                let idx = *vindex.entry(k).or_insert_with(|| {
                    positions.push(p);
                    normals.push(nn);
                    (positions.len() - 1) as u32
                });
                indices.push(idx);
            }
        }
        if positions.is_empty() {
            continue;
        }
        let mut mn = [f64::INFINITY; 3];
        let mut mx = [f64::NEG_INFINITY; 3];
        for p in &positions {
            for k in 0..3 {
                mn[k] = mn[k].min(p[k]);
                mx[k] = mx[k].max(p[k]);
            }
        }
        let pos_acc = b.accessor(
            &f32s(&positions),
            F32,
            positions.len(),
            "VEC3",
            Some(ARRAY_BUFFER),
            Some((mn, mx)),
        );
        let nrm_acc = b.accessor(
            &f32s(&normals),
            F32,
            normals.len(),
            "VEC3",
            Some(ARRAY_BUFFER),
            None,
        );
        let idx_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let idx_acc = b.accessor(
            &idx_bytes,
            U32,
            indices.len(),
            "SCALAR",
            Some(ELEMENT_ARRAY_BUFFER),
            None,
        );
        primitives.push(json!({
            "attributes": {"POSITION": pos_acc, "NORMAL": nrm_acc},
            "indices": idx_acc, "material": mat_index[key], "mode": 4
        }));
    }
    let mut gltf = Map::new();
    gltf.insert(
        "asset".into(),
        json!({"version": "2.0", "generator": "Xtrude CAD", "extras": {"generator": "Xtrude CAD"}}),
    );
    let mut n_nodes = 0;
    if !primitives.is_empty() {
        gltf.insert(
            "meshes".into(),
            json!([{"name": "Xtrude CAD", "primitives": primitives}]),
        );
        gltf.insert("nodes".into(), json!([{"mesh": 0, "name": "Xtrude CAD"}]));
        n_nodes = 1;
    }
    gltf.insert(
        "scenes".into(),
        json!([{"nodes": (0..n_nodes).collect::<Vec<_>>()}]),
    );
    gltf.insert("scene".into(), json!(0));
    if !b.accessors.is_empty() {
        gltf.insert("accessors".into(), Value::Array(b.accessors));
        gltf.insert("bufferViews".into(), Value::Array(b.views));
        gltf.insert("buffers".into(), json!([{"byteLength": b.buf.len()}]));
    }
    if !gltf_materials.is_empty() {
        gltf.insert("materials".into(), Value::Array(gltf_materials));
    }
    let mut json_bytes = serde_json::to_vec(&Value::Object(gltf)).map_err(std::io::Error::other)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin = b.buf;
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    for v in [
        0x4654_6C67u32,
        2,
        total as u32,
        json_bytes.len() as u32,
        0x4E4F_534A,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
    }
    std::fs::write(path, out)
}
