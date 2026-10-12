// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Wavefront OBJ import/export, with per-face colour as `.mtl` materials.
//!
//! Export: vertices de-duplicated by position, every face triangulated,
//! triangles grouped by their colour (`usemtl`). Import: polygons welded
//! and fused back into the faces they were exported from; a file made of
//! named parts arrives as a container of parts; big files become a
//! reference group.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use xtrude_core::attrs::{Back, FaceAttrs, Rgb};
use xtrude_core::fuse::{
    drop_smoothing_groups, fuse_coplanar_loops, soften_by_smoothing_group, soften_smooth_edges,
    Fused, Loop,
};
use xtrude_core::math::DMat4;
use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::stable_json::round_exact;
use xtrude_core::stitch::run_stitch;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::group::Group;
use xtrude_model::i18n::tr;
use xtrude_model::scene::Scene;

use crate::meshexport::{export_names, world_faces, MaterialInfo, MaterialKey, DEFAULT_COLOR};
use crate::stl::MAX_FUSE_LOOPS;

/// Metres per file unit.
pub const OBJ_UNITS: [(&str, f64); 5] = [
    ("m", 1.0),
    ("cm", 0.01),
    ("mm", 0.001),
    ("in", 0.0254),
    ("ft", 0.3048),
];

/// Row-major 3×3 that stands a Y-up file up.
pub const Y_UP_TO_Z_UP: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0];

#[derive(Debug, thiserror::Error)]
pub enum ObjError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

/// Write the scene as `path` (.obj) plus a sibling `.mtl`; face colours
/// become `Kd` materials.
pub fn save_obj(scene: &Scene, path: &Path) -> Result<(), ObjError> {
    let mut verts: Vec<DVec3> = Vec::new();
    let mut vindex: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let mut vidx = |p: DVec3| -> usize {
        let k = (
            round_exact(p.x, 6).to_bits(),
            round_exact(p.y, 6).to_bits(),
            round_exact(p.z, 6).to_bits(),
        );
        *vindex.entry(k).or_insert_with(|| {
            verts.push(p);
            verts.len() // OBJ indices are 1-based
        })
    };
    let mut materials: IndexMap<MaterialKey, MaterialInfo> = IndexMap::new();
    let mut groups: IndexMap<MaterialKey, Vec<[usize; 3]>> = IndexMap::new();
    for face in world_faces(scene, false) {
        let col = face.attrs.paint.color.unwrap_or(DEFAULT_COLOR);
        let key = MaterialKey {
            color: col,
            opacity: None,
        };
        let info = materials
            .entry(key.clone())
            .or_insert_with(|| MaterialInfo {
                color: col,
                opacity: None,
                mat: None,
            });
        if info.mat.is_none() {
            if let Some(m) = face.attrs.paint.mat.as_ref().filter(|m| !m.is_empty()) {
                info.mat = Some(m.clone());
            }
        }
        for t in &face.tris {
            let tri = [vidx(t[0]), vidx(t[1]), vidx(t[2])];
            groups.entry(key.clone()).or_default().push(tri);
        }
    }
    let used: IndexMap<MaterialKey, MaterialInfo> = groups
        .keys()
        .map(|k| (k.clone(), materials[k].clone()))
        .collect();
    let names = export_names(&used);
    let mtl_path = path.with_extension("mtl");
    let mut mtl = String::new();
    for k in groups.keys() {
        let [r, g, b] = used[k].color;
        mtl += &format!("newmtl {}\nKd {r:.4} {g:.4} {b:.4}\n\n", names[k]);
    }
    std::fs::write(&mtl_path, mtl)?;
    let mtl_name = mtl_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = format!("# Xtrude CAD OBJ export\nmtllib {mtl_name}\n");
    for p in &verts {
        out += &format!("v {:.6} {:.6} {:.6}\n", p.x, p.y, p.z);
    }
    for (k, tris) in &groups {
        out += &format!("usemtl {}\n", names[k]);
        for t in tris {
            out += &format!("f {} {} {}\n", t[0], t[1], t[2]);
        }
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Material name → `Kd` colour, from a `.mtl` file (a `map_Kd` image is not
/// imported: a textured material keeps its `Kd` colour).
fn parse_mtl(path: &Path) -> Result<HashMap<String, Option<Rgb>>, ObjError> {
    let mut mats = HashMap::new();
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(mats);
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut current: Option<String> = None;
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        if parts[0] == "newmtl" {
            let name = parts.get(1).copied().unwrap_or("").to_string();
            mats.insert(name.clone(), None);
            current = Some(name);
        } else if let Some(cur) = &current {
            if parts[0] == "Kd" && parts.len() >= 4 {
                let c = |i: usize| -> Result<f64, ObjError> {
                    parts[i]
                        .parse()
                        .map_err(|_| ObjError::Invalid(format!("bad Kd value {:?}", parts[i])))
                };
                mats.insert(cur.clone(), Some([c(1)?, c(2)?, c(3)?]));
            }
        }
    }
    Ok(mats)
}

/// The unit an OBJ was most likely written in, from its own size: over
/// 20000 units across is millimetres, over 200 centimetres, else metres.
pub fn suggest_unit(path: &Path) -> &'static str {
    let Ok(bytes) = std::fs::read(path) else {
        return "m";
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for line in bytes.split(|&b| b == b'\n') {
        if !line.starts_with(b"v ") {
            continue;
        }
        let text = String::from_utf8_lossy(line);
        let p: Vec<&str> = text.split_whitespace().collect();
        if p.len() < 4 {
            continue;
        }
        for k in 0..3 {
            let Ok(v) = p[k + 1].parse::<f64>() else {
                return "m";
            };
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }
    let span = if hi[0] > lo[0] {
        (0..3).map(|k| hi[k] - lo[k]).fold(f64::MIN, f64::max)
    } else {
        0.0
    };
    if span > 20000.0 {
        "mm"
    } else if span > 200.0 {
        "cm"
    } else {
        "m"
    }
}

struct Pending {
    loop_: Vec<DVec3>,
    mat: Option<Option<Rgb>>,
    smooth: i64,
    piece: (String, String),
}

fn face_attrs(mat: &Option<Option<Rgb>>, smooth: i64) -> Option<FaceAttrs> {
    let mut a = FaceAttrs::default();
    if smooth != 0 {
        a.smooth = Some(smooth.to_string());
    }
    if let Some(Some(color)) = mat {
        let r4 = |c: &Rgb| c.map(|v| round_exact(v, 4));
        if r4(color) != r4(&DEFAULT_COLOR) {
            a.paint.color = Some(*color);
            a.back = Some(Back::Mirror);
        }
    }
    if a.is_empty() {
        None
    } else {
        Some(a)
    }
}

/// Add `fuse_coplanar_loops` output to `mesh`.
pub fn add_fused(mesh: &mut Mesh, fused: Vec<Fused>) {
    for (outer, holes, attrs, _originals) in fused {
        let f = mesh.add_face(&outer, &holes);
        if let Some(a) = attrs {
            mesh.face_mut(f).attrs.update(&a);
        }
    }
}

pub fn tidy_blender_name(name: &str) -> String {
    let mut out = name.to_string();
    if let Some((head, tail)) = name.rsplit_once('_') {
        // a capitalised word, optionally with a copy number: Cube, Cube.007
        let (word, num) = match tail.split_once('.') {
            Some((w, n)) => (w, Some(n)),
            None => (tail, None),
        };
        let word_ok = word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && word.chars().all(|c| c.is_ascii_alphabetic());
        let num_ok = num.is_none_or(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if !head.is_empty() && word_ok && num_ok {
            out = head.to_string();
        }
    }
    // strip trailing (.ddd)+
    let mut s = out.as_str();
    loop {
        let b = s.as_bytes();
        if b.len() >= 4 && b[b.len() - 4] == b'.' && b[b.len() - 3..].iter().all(u8::is_ascii_digit)
        {
            s = &s[..s.len() - 4];
        } else {
            break;
        }
    }
    let trimmed = s.trim();
    if trimmed.is_empty() {
        out
    } else {
        trimmed.to_string()
    }
}

fn piece_name(key: &(String, String), number: usize) -> String {
    let (obj_name, group_name) = key;
    let raw = if group_name.is_empty() {
        obj_name
    } else {
        group_name
    };
    let name = tidy_blender_name(raw.trim());
    if name.is_empty() {
        return tr("Part {n}", &[("n", &number)]);
    }
    if !name.chars().any(char::is_alphabetic) {
        return tr("Part {n}", &[("n", &name)]);
    }
    name
}

fn finish_mesh(mesh: &mut Mesh) {
    soften_by_smoothing_group(mesh, 0.17);
    soften_smooth_edges(mesh, 0.85);
    drop_smoothing_groups(mesh);
}

fn pieces_group(
    scene: &mut Scene,
    stem: &str,
    pieces: &IndexMap<(String, String), Vec<&Pending>>,
    tick: &mut dyn FnMut(f64, &str),
) -> Group {
    let mut kids = Vec::new();
    let n = pieces.len().max(1);
    for (k, (key, items)) in pieces.iter().enumerate() {
        let k = k + 1;
        tick(0.5 + 0.45 * k as f64 / n as f64, "Building the model…");
        let raw: Vec<Loop> = items
            .iter()
            .map(|p| (p.loop_.clone(), face_attrs(&p.mat, p.smooth)))
            .collect();
        let mut mesh = Mesh::new();
        add_fused(&mut mesh, fuse_coplanar_loops(&raw, 0.99999, false));
        if mesh.faces.is_empty() {
            continue;
        }
        finish_mesh(&mut mesh);
        let id = scene.doc.add_mesh(mesh);
        let mut kid = Group::new(id, Some(&piece_name(key, k)));
        kid.xform = Some(DMat4::IDENTITY);
        kid.component = false; // a part is a group inside the component
        kids.push(kid);
    }
    let own = scene.doc.add_mesh(Mesh::new());
    let mut container = Group::new(own, Some(stem));
    container.adopt(kids);
    container
}

/// Add the faces of a Wavefront OBJ to `scene` (the caller wraps it for
/// undo). `scale` is metres per file unit; `matrix` an optional row-major
/// 3×3 applied after it (e.g. [`Y_UP_TO_Z_UP`]).
pub fn load_obj(
    scene: &mut Scene,
    path: &Path,
    mut progress: Option<Progress<'_>>,
    scale: f64,
    matrix: Option<[f64; 9]>,
) -> Result<(), ObjError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    tick(0.05, "Reading file…");
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let mut verts: Vec<DVec3> = Vec::new();
    let mut materials: HashMap<String, Option<Rgb>> = HashMap::new();
    let mut current_mat: Option<Option<Rgb>> = None;
    let mut current_smooth = 0i64;
    let (mut current_o, mut current_g) = (String::new(), String::new());
    let mut pending: Vec<Pending> = Vec::new();
    let num = |s: &str| -> Result<f64, ObjError> {
        s.parse()
            .map_err(|_| ObjError::Invalid(format!("could not convert string to float: {s:?}")))
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "v" => {
                if parts.len() < 4 {
                    return Err(ObjError::Invalid(format!(
                        "vertex line too short: {line:?}"
                    )));
                }
                let (mut x, mut y, mut z) = (
                    num(parts[1])? * scale,
                    num(parts[2])? * scale,
                    num(parts[3])? * scale,
                );
                if let Some(m) = matrix {
                    (x, y, z) = (
                        m[0] * x + m[1] * y + m[2] * z,
                        m[3] * x + m[4] * y + m[5] * z,
                        m[6] * x + m[7] * y + m[8] * z,
                    );
                }
                verts.push(DVec3::new(x, y, z));
            }
            "mtllib" => {
                if let Some(name) = parts.get(1) {
                    materials = parse_mtl(&path.with_file_name(name))?;
                }
            }
            "usemtl" => current_mat = parts.get(1).and_then(|n| materials.get(*n).cloned()),
            "o" => {
                current_o = parts[1..].join(" ");
                current_g.clear();
            }
            "g" => current_g = parts[1..].join(" "),
            "s" => {
                let v = parts
                    .get(1)
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| "off".into());
                current_smooth = if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                    v.parse().unwrap_or(0)
                } else {
                    0
                };
            }
            "f" => {
                let mut idxs = Vec::new();
                let mut ok = true;
                for tok in &parts[1..] {
                    match tok.split('/').next().unwrap_or("").parse::<i64>() {
                        Ok(raw) => idxs.push(if raw > 0 {
                            raw - 1
                        } else {
                            verts.len() as i64 + raw
                        }),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok
                    && idxs.len() >= 3
                    && idxs.iter().all(|&i| 0 <= i && (i as usize) < verts.len())
                {
                    pending.push(Pending {
                        loop_: idxs.iter().map(|&i| verts[i as usize]).collect(),
                        mat: current_mat,
                        smooth: current_smooth,
                        piece: (current_o.clone(), current_g.clone()),
                    });
                }
            }
            _ => {}
        }
    }

    let mut pieces: IndexMap<(String, String), Vec<&Pending>> = IndexMap::new();
    for p in &pending {
        pieces.entry(p.piece.clone()).or_default().push(p);
    }
    if pieces.len() > 1 {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let g = pieces_group(scene, &stem, &pieces, &mut tick);
        scene.doc.groups.push(g);
        scene.version += 1;
        tick(1.0, "Done");
        return Ok(());
    }

    if pending.len() > MAX_FUSE_LOOPS {
        let raw: Vec<Loop> = pending
            .iter()
            .map(|p| (p.loop_.clone(), face_attrs(&p.mat, p.smooth)))
            .collect();
        tick(0.5, "Merging coplanar faces…");
        let fused = fuse_coplanar_loops(&raw, 0.99999, false);
        let n = fused.len().max(1);
        let mut target = Mesh::new();
        for (k, item) in fused.into_iter().enumerate() {
            if k % 8192 == 0 {
                tick(0.6 + 0.3 * k as f64 / n as f64, "Building the model…");
            }
            add_fused(&mut target, vec![item]);
        }
        tick(0.92, "Smoothing edges…");
        finish_mesh(&mut target);
        let id = scene.doc.add_mesh(target);
        scene.doc.groups.push(Group::new(id, None));
        scene.version += 1;
        tick(1.0, "Done");
        return Ok(());
    }

    let mut seed: Vec<DVec3> = Vec::new();
    let mut new_faces: HashSet<FaceId> = HashSet::new();
    {
        let target = scene.mesh_mut();
        for p in &pending {
            let f = target.add_face(&p.loop_, &[]);
            new_faces.insert(f);
            if let Some(a) = face_attrs(&p.mat, p.smooth) {
                target.face_mut(f).attrs.update(&a);
            }
            seed.extend(p.loop_.iter().copied());
        }
        run_stitch(target, &seed, Some(&mut new_faces), true, true);
        orient_outward(target, DEFAULT_SEED, None);
        soften_by_smoothing_group(target, 0.17);
        drop_smoothing_groups(target);
    }
    scene.version += 1;
    Ok(())
}
