// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! glTF 2.0 / GLB import — the common binary 3D exchange format.
//!
//! Reads a binary `.glb` (or JSON `.gltf` with external or data-URI
//! buffers) and adds its geometry as a *reference* group: triangles
//! bulk-welded in world Z-up metres, each material's base colour on its
//! faces (textures are not imported), facet seams softened. Supported:
//! TRIANGLES primitives, u8/u16/u32 indices, float/normalized-int
//! attributes with byteStride, node hierarchies (matrix or TRS), the
//! KHR_materials_pbrSpecularGlossiness legacy, alphaMode BLEND → opacity.
//! Not supported: Draco (clear error), skinning, sparse accessors.

use std::path::Path;

use base64::Engine;
use serde_json::Value;
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::fuse::soften_smooth_edges;
use xtrude_core::math::{DMat4, DVec4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::Group;
use xtrude_model::scene::Scene;

#[derive(Debug, thiserror::Error)]
pub enum GlbError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(m: impl Into<String>) -> GlbError {
    GlbError::Invalid(m.into())
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

/// `(gltf json, buffers)` of a `.glb` or `.gltf`.
pub fn parse_container(path: &Path) -> Result<(Value, Vec<Vec<u8>>), GlbError> {
    let data = std::fs::read(path)?;
    let (js, default) = if data.starts_with(b"glTF") {
        let mut off = 12usize;
        let mut js = None;
        let mut bin = Vec::new();
        while off + 8 <= data.len() {
            let clen = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
                as usize;
            let ctype =
                u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
            let end = (off + 8 + clen).min(data.len());
            let chunk = &data[off + 8..end];
            if ctype == 0x4E4F_534A {
                js = Some(serde_json::from_slice::<Value>(chunk)?);
            } else if ctype == 0x004E_4942 {
                bin = chunk.to_vec();
            }
            off += 8 + clen + (4 - clen % 4) % 4;
        }
        (js.ok_or_else(|| invalid("GLB without a JSON chunk"))?, bin)
    } else {
        (serde_json::from_slice::<Value>(&data)?, Vec::new())
    };
    let mut buffers = Vec::new();
    for buf in js
        .get("buffers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match buf.get("uri").and_then(Value::as_str) {
            None => buffers.push(default.clone()),
            Some(uri) if uri.starts_with("data:") => {
                let b64 = uri.split_once(',').map(|x| x.1).unwrap_or("");
                buffers.push(
                    base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|e| invalid(format!("bad data URI: {e}")))?,
                );
            }
            Some(uri) => buffers.push(std::fs::read(path.with_file_name(uri))?),
        }
    }
    Ok((js, buffers))
}

fn get_usize(v: &Value, k: &str) -> Option<usize> {
    v.get(k).and_then(Value::as_u64).map(|x| x as usize)
}

/// Accessor `idx` as rows of `f64` (indices come back as whole numbers).
fn accessor(
    js: &Value,
    buffers: &[Vec<u8>],
    idx: usize,
) -> Result<(Vec<Vec<f64>>, bool), GlbError> {
    let acc = js["accessors"]
        .get(idx)
        .ok_or_else(|| invalid("bad accessor index"))?;
    if acc.get("sparse").is_some() {
        return Err(invalid("sparse accessors are not supported"));
    }
    let comp = acc
        .get("componentType")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let (size, signed, float) = match comp {
        5120 => (1, true, false),
        5121 => (1, false, false),
        5122 => (2, true, false),
        5123 => (2, false, false),
        5125 => (4, false, false),
        5126 => (4, false, true),
        _ => return Err(invalid(format!("unknown componentType {comp}"))),
    };
    let n = match acc.get("type").and_then(Value::as_str).unwrap_or("") {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        t => return Err(invalid(format!("unknown accessor type {t:?}"))),
    };
    let count = get_usize(acc, "count").unwrap_or(0);
    let bv = js["bufferViews"]
        .get(get_usize(acc, "bufferView").unwrap_or(0))
        .ok_or_else(|| invalid("bad bufferView"))?;
    let buf = buffers
        .get(get_usize(bv, "buffer").unwrap_or(0))
        .ok_or_else(|| invalid("bad buffer"))?;
    let start =
        get_usize(bv, "byteOffset").unwrap_or(0) + get_usize(acc, "byteOffset").unwrap_or(0);
    let stride = get_usize(bv, "byteStride")
        .filter(|&s| s > 0)
        .unwrap_or(size * n);
    let as_int = comp == 5125 || (n == 1 && matches!(comp, 5121 | 5123));
    let normalized = acc
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let peak = match comp {
        5120 => 127.0,
        5121 => 255.0,
        5122 => 32767.0,
        5123 => 65535.0,
        _ => 0.0,
    };
    let mut rows = Vec::with_capacity(count);
    for r in 0..count {
        let base = start + r * stride;
        let mut row = Vec::with_capacity(n);
        for c in 0..n {
            let o = base + c * size;
            let b = buf
                .get(o..o + size)
                .ok_or_else(|| invalid("accessor reads past its buffer"))?;
            let v = match (size, signed, float) {
                (4, _, true) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                (4, _, false) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                (2, true, _) => i16::from_le_bytes([b[0], b[1]]) as f64,
                (2, false, _) => u16::from_le_bytes([b[0], b[1]]) as f64,
                (1, true, _) => b[0] as i8 as f64,
                _ => b[0] as f64,
            };
            row.push(if !as_int && normalized && peak > 0.0 {
                v / peak
            } else {
                v
            });
        }
        rows.push(row);
    }
    Ok((rows, as_int))
}

fn arr<const N: usize>(v: &Value) -> Option<[f64; N]> {
    let a = v.as_array()?;
    if a.len() < N {
        return None;
    }
    let mut out = [0.0; N];
    for i in 0..N {
        out[i] = a[i].as_f64()?;
    }
    Some(out)
}

fn local(nd: &Value) -> DMat4 {
    if let Some(m) = nd.get("matrix").and_then(arr::<16>) {
        return DMat4::from_cols_array(&m); // glTF is column-major
    }
    let mut m = DMat4::IDENTITY;
    if let Some(s) = nd.get("scale").and_then(arr::<3>) {
        m *= DMat4::from_scale(DVec3::from_array(s));
    }
    if let Some([x, y, z, w]) = nd.get("rotation").and_then(arr::<4>) {
        let r = DMat4::from_cols(
            DVec4::new(
                1. - 2. * (y * y + z * z),
                2. * (x * y + z * w),
                2. * (x * z - y * w),
                0.,
            ),
            DVec4::new(
                2. * (x * y - z * w),
                1. - 2. * (x * x + z * z),
                2. * (y * z + x * w),
                0.,
            ),
            DVec4::new(
                2. * (x * z + y * w),
                2. * (y * z - x * w),
                1. - 2. * (x * x + y * y),
                0.,
            ),
            DVec4::W,
        );
        m = r * m;
    }
    if let Some(t) = nd.get("translation").and_then(arr::<3>) {
        m = DMat4::from_translation(DVec3::from_array(t)) * m;
    }
    m
}

/// `(node index, world matrix)` for every node down the scene hierarchy.
fn node_transforms(js: &Value) -> Vec<(usize, DMat4)> {
    let nodes = js
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut roots: Vec<usize> = Vec::new();
    let scenes = js
        .get("scenes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for sc in scenes {
        roots.extend(
            sc.get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_u64())
                .map(|v| v as usize),
        );
    }
    if roots.is_empty() {
        roots = (0..nodes.len()).collect();
    }
    let mut out = Vec::new();
    fn walk(nodes: &[Value], i: usize, parent: DMat4, out: &mut Vec<(usize, DMat4)>, depth: usize) {
        let Some(nd) = nodes.get(i) else { return };
        if depth > 64 {
            return;
        }
        let world = parent * local(nd);
        out.push((i, world));
        for c in nd
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            walk(nodes, c as usize, world, out, depth + 1);
        }
    }
    for r in roots {
        walk(&nodes, r, DMat4::IDENTITY, &mut out, 0);
    }
    out
}

type Recipe = (Option<Vec<f64>>, Option<f64>);

fn material_recipe(js: &Value, mat_idx: Option<usize>) -> Recipe {
    let Some(mat) = mat_idx.and_then(|i| js.get("materials").and_then(|m| m.get(i))) else {
        return (None, None);
    };
    let sg = mat
        .get("extensions")
        .and_then(|e| e.get("KHR_materials_pbrSpecularGlossiness"));
    let rgba: Option<Vec<f64>> = match sg {
        Some(sg) => sg.get("diffuseFactor"),
        None => mat
            .get("pbrMetallicRoughness")
            .and_then(|p| p.get("baseColorFactor")),
    }
    .and_then(Value::as_array)
    .map(|a| a.iter().filter_map(Value::as_f64).collect());
    let mut opacity = None;
    if mat.get("alphaMode").and_then(Value::as_str) == Some("BLEND") {
        let a = rgba.as_ref().filter(|c| c.len() > 3).map_or(1.0, |c| c[3]);
        opacity = Some(if a < 0.999 { a } else { 0.999 });
    }
    (rgba, opacity)
}

/// One primitive's world-space Z-up triangles and its material recipe.
pub type Batch = (Vec<[DVec3; 3]>, Recipe);

/// Parse the file into world-space Z-up triangle batches.
pub fn read_triangles(path: &Path) -> Result<(Vec<Batch>, Value), GlbError> {
    let (js, buffers) = parse_container(path)?;
    for ext in js
        .get("extensionsRequired")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if ext.to_lowercase().contains("draco") {
            return Err(invalid(
                "Draco-compressed GLB — re-export/download uncompressed",
            ));
        }
    }
    let zup = |p: DVec3| DVec3::new(p.x, -p.z, p.y);
    let meshes = js
        .get("meshes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut batches = Vec::new();
    for (ni, world) in node_transforms(&js) {
        let Some(mi) = js["nodes"][ni]
            .get("mesh")
            .and_then(Value::as_u64)
            .map(|m| m as usize)
        else {
            continue;
        };
        let Some(mesh) = meshes.get(mi) else { continue };
        for prim in mesh
            .get("primitives")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if prim.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
                continue;
            }
            let Some(pi) = prim
                .get("attributes")
                .and_then(|a| a.get("POSITION"))
                .and_then(Value::as_u64)
            else {
                continue;
            };
            let (pos, _) = accessor(&js, &buffers, pi as usize)?;
            let idx: Vec<usize> = match prim.get("indices").and_then(Value::as_u64) {
                Some(ii) => accessor(&js, &buffers, ii as usize)?
                    .0
                    .iter()
                    .map(|r| r[0] as usize)
                    .collect(),
                None => (0..pos.len()).collect(),
            };
            let recipe = material_recipe(
                &js,
                prim.get("material")
                    .and_then(Value::as_u64)
                    .map(|m| m as usize),
            );
            let world_pts: Vec<DVec3> = pos
                .iter()
                .map(|r| {
                    let p = DVec3::new(r[0], *r.get(1).unwrap_or(&0.0), *r.get(2).unwrap_or(&0.0));
                    zup(world.transform_point3(p))
                })
                .collect();
            let mut tris = Vec::with_capacity(idx.len() / 3);
            for t in idx.chunks_exact(3) {
                if t.iter().any(|&i| i >= world_pts.len()) {
                    return Err(invalid("index out of range"));
                }
                tris.push([world_pts[t[0]], world_pts[t[1]], world_pts[t[2]]]);
            }
            batches.push((tris, recipe));
        }
    }
    Ok((batches, js))
}

const SOFTEN_EDGE_CAP: usize = 500_000;

/// One mesh from the batches: bulk-welded, coloured, seams softened.
pub fn build_mesh(
    batches: &[Batch],
    progress: &mut Option<Progress<'_>>,
) -> Result<Mesh, GlbError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    let mut mesh = Mesh::new();
    let mut flat: Vec<DVec3> = Vec::new();
    let mut attrs: Vec<Option<FaceAttrs>> = Vec::new();
    let total = batches.iter().map(|b| b.0.len()).sum::<usize>().max(1);
    let mut done = 0;
    for (tris, (rgba, opacity)) in batches {
        flat.extend(tris.iter().flatten().copied());
        let color = rgba
            .as_ref()
            .filter(|c| c.len() >= 3)
            .map(|c| [c[0], c[1], c[2]]);
        let mut base = FaceAttrs::default();
        if let Some(c) = color.filter(|c| *c != [1.0, 1.0, 1.0]) {
            base.paint = Paint {
                color: Some(c),
                ..Default::default()
            };
            base.back = Some(Back::Mirror);
        }
        if let Some(o) = opacity {
            base.paint.opacity = Some(*o);
        }
        let a = if base.is_empty() { None } else { Some(base) };
        attrs.extend(std::iter::repeat_n(a, tris.len()));
        done += tris.len();
        tick(
            0.3 + 0.4 * done as f64 / total as f64,
            "Preparing geometry…",
        );
    }
    if flat.is_empty() {
        return Err(invalid("No triangle geometry found in the glTF file"));
    }
    let (vobjs, ids) = mesh.bulk_weld(&flat);
    let n = flat.len() / 3;
    tick(0.75, "Building the model…");
    mesh.add_faces_welded(&vobjs, &ids, &vec![3; n], &vec![1; n], &attrs);
    if mesh.edges.len() <= SOFTEN_EDGE_CAP {
        tick(0.9, "Smoothing seams…");
        soften_smooth_edges(&mut mesh, 0.85);
    }
    Ok(mesh)
}

/// glTF says metres, authors say whatever: the first of metres / inches /
/// centimetres / millimetres that lands the largest dimension in 0.2–40 m.
fn guess_scale(batches: &[Batch]) -> f64 {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for (tris, _) in batches {
        for p in tris.iter().flatten() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    let dim = (hi - lo).max_element();
    if dim.is_nan() || dim <= 0.0 {
        return 1.0;
    }
    for s in [1.0, 0.0254, 0.01, 0.001] {
        if (0.2..=40.0).contains(&(dim * s)) {
            return s;
        }
    }
    1.0
}

/// Add the glTF/GLB at `path` to `scene` as a reference group named after
/// the file (or the embedded Sketchfab title).
pub fn load_glb(
    scene: &mut Scene,
    path: &Path,
    mut progress: Option<Progress<'_>>,
) -> Result<(), GlbError> {
    if let Some(p) = progress.as_mut() {
        p(0.05, "Reading file…");
    }
    let (mut batches, js) = read_triangles(path)?;
    let s = guess_scale(&batches);
    if s != 1.0 {
        for (tris, _) in batches.iter_mut() {
            for t in tris.iter_mut() {
                *t = t.map(|p| p * s);
            }
        }
    }
    let mesh = build_mesh(&batches, &mut progress)?;
    let title = js
        .get("asset")
        .and_then(|a| a.get("extras"))
        .and_then(|e| e.get("title"))
        .and_then(Value::as_str);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name: String = title
        .filter(|t| !t.is_empty())
        .unwrap_or(&stem)
        .chars()
        .take(60)
        .collect();
    let id = scene.doc.add_mesh(mesh);
    scene.doc.groups.push(Group::new(id, Some(&name)));
    Ok(())
}
