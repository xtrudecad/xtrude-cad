// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STL import/export — triangle soup for 3D printing and mesh interchange.
//!
//! Binary STL: every face (the loose mesh plus every group) is triangulated
//! and written with its geometric normal. STL carries no units: every
//! slicer reads millimetres, so the export writes millimetres (the model is
//! stored in metres). Import asks for the unit and suggests millimetres.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::stitch::run_stitch;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::group::Group;
use xtrude_model::scene::Scene;

use crate::meshexport::world_faces;

/// Metres per file unit.
pub const STL_UNITS: [(&str, f64); 5] = [
    ("m", 1.0),
    ("cm", 0.01),
    ("mm", 0.001),
    ("in", 0.0254),
    ("ft", 0.3048),
];

/// Metres per unit of `unit`, if known.
pub fn unit_scale(unit: &str) -> Option<f64> {
    STL_UNITS.iter().find(|(u, _)| *u == unit).map(|(_, s)| *s)
}

const BINARY_HEADER_SIZE: u64 = 84;
const BINARY_TRIANGLE_SIZE: u64 = 50;
const MAX_FLOAT32: f64 = 3.4028234663852886e38;
const BATCH_SIZE: usize = 4096;

/// Above this many faces an import is kept as a reference group instead of
/// being stitched into the loose mesh.
pub const MAX_FUSE_LOOPS: usize = 400;

#[derive(Debug, thiserror::Error)]
pub enum StlError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> StlError {
    StlError::Invalid(msg.into())
}

/// How coplanar facets are merged on import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimplifyMode {
    None,
    /// XY / XZ / YZ planes only.
    Principal,
    /// Coplanar surfaces in any orientation.
    All,
}

impl SimplifyMode {
    pub fn parse(s: &str) -> Option<SimplifyMode> {
        match s {
            "none" => Some(SimplifyMode::None),
            "principal" => Some(SimplifyMode::Principal),
            "all" => Some(SimplifyMode::All),
            _ => None,
        }
    }
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

fn tick(progress: &mut Option<Progress<'_>>, f: f64, msg: &str) {
    if let Some(p) = progress.as_mut() {
        p(f, msg);
    }
}

/// Every world-space triangle of the scene.
pub fn iter_triangles(scene: &Scene) -> Vec<[DVec3; 3]> {
    world_faces(scene, false)
        .into_iter()
        .flat_map(|f| f.tris)
        .collect()
}

fn scaled_point(values: [f64; 3], scale: f64) -> Result<[f64; 3], String> {
    let p = values.map(|v| v * scale);
    if p.iter().all(|v| v.is_finite() && v.abs() <= MAX_FLOAT32) {
        Ok(p)
    } else {
        Err("STL contains a coordinate outside the supported range".into())
    }
}

fn non_degenerate(t: &[[f64; 3]; 3]) -> bool {
    let [a, b, c] = t;
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() > 1e-12
}

fn parse_floats(parts: &[&str]) -> Option<Vec<f64>> {
    parts.iter().map(|p| parse_float(p)).collect()
}

/// A float token (`nan`, `inf`, `infinity` included).
fn parse_float(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}

fn ascii_triangles(
    path: &Path,
    scale: f64,
    progress: &mut Option<Progress<'_>>,
    file_size: u64,
    mut sink: impl FnMut([[f64; 3]; 3]),
) -> Result<(), StlError> {
    #[derive(PartialEq)]
    enum St {
        Outside,
        Outer,
        Vertices,
        EndLoop,
        EndFacet,
    }
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut state = St::Outside;
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut line_number = 0u64;
    let mut read_bytes = 0u64;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        read_bytes += n as u64;
        line_number += 1;
        let mut line = String::from_utf8_lossy(&buf).into_owned();
        if line_number == 1 {
            line = line.trim_start_matches('\u{feff}').to_string();
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let keyword = parts[0].to_lowercase();
        match state {
            St::Outside => {
                if keyword == "facet" {
                    if parts.len() != 5 || parts[1].to_lowercase() != "normal" {
                        return Err(invalid(format!(
                            "Invalid STL facet normal on line {line_number}"
                        )));
                    }
                    let normal = parse_floats(&parts[2..]).ok_or_else(|| {
                        invalid(format!("Invalid STL facet normal on line {line_number}"))
                    })?;
                    if !normal.iter().all(|v| v.is_finite()) {
                        return Err(invalid(format!(
                            "Non-finite STL facet normal on line {line_number}"
                        )));
                    }
                    verts.clear();
                    state = St::Outer;
                }
            }
            St::Outer => {
                if parts.len() != 2 || keyword != "outer" || parts[1].to_lowercase() != "loop" {
                    return Err(invalid(format!(
                        "Expected 'outer loop' on STL line {line_number}"
                    )));
                }
                state = St::Vertices;
            }
            St::Vertices => {
                if keyword != "vertex" || parts.len() != 4 {
                    return Err(invalid(format!(
                        "Expected an STL vertex on line {line_number}"
                    )));
                }
                let vals = parse_floats(&parts[1..]).ok_or_else(|| {
                    invalid(format!(
                        "Invalid STL vertex on line {line_number}: could not convert string to float"
                    ))
                })?;
                let p = scaled_point([vals[0], vals[1], vals[2]], scale).map_err(|e| {
                    invalid(format!("Invalid STL vertex on line {line_number}: {e}"))
                })?;
                verts.push(p);
                if verts.len() == 3 {
                    state = St::EndLoop;
                }
            }
            St::EndLoop => {
                if keyword != "endloop" || parts.len() != 1 {
                    return Err(invalid(format!(
                        "Expected 'endloop' on STL line {line_number}"
                    )));
                }
                state = St::EndFacet;
            }
            St::EndFacet => {
                if keyword != "endfacet" || parts.len() != 1 {
                    return Err(invalid(format!(
                        "Expected 'endfacet' on STL line {line_number}"
                    )));
                }
                sink([verts[0], verts[1], verts[2]]);
                state = St::Outside;
            }
        }
        if line_number % 4096 == 0 {
            let fraction = (read_bytes as f64 / file_size.max(1) as f64).min(1.0);
            tick(progress, 0.05 + 0.65 * fraction, "Reading file…");
        }
    }
    if state != St::Outside {
        return Err(invalid("STL ASCII facet is incomplete"));
    }
    Ok(())
}

fn flush(target: &mut Mesh, batch: &mut Vec<[[f64; 3]; 3]>) {
    if batch.is_empty() {
        return;
    }
    let pos: Vec<DVec3> = batch
        .iter()
        .flat_map(|t| t.iter().map(|p| DVec3::from_array(*p)))
        .collect();
    let n = batch.len();
    let (vobjs, ids) = target.bulk_weld(&pos);
    target.add_faces_welded(&vobjs, &ids, &vec![3; n], &vec![1; n], &[]);
    batch.clear();
}

/// Parse and prepare STL geometry without touching a scene (safe on a
/// worker thread). `scale` is metres per file unit.
pub fn parse_stl(
    path: &Path,
    mut progress: Option<Progress<'_>>,
    scale: f64,
    simplify: SimplifyMode,
) -> Result<Mesh, StlError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid("STL unit scale must be a positive finite number"));
    }
    tick(&mut progress, 0.02, "Reading file…");
    let size = std::fs::metadata(path)?.len();
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; BINARY_HEADER_SIZE as usize];
    let mut binary = false;
    let mut count = 0u64;
    if file.read_exact(&mut header).is_ok() {
        count = u32::from_le_bytes([header[80], header[81], header[82], header[83]]) as u64;
        let expected = BINARY_HEADER_SIZE + count * BINARY_TRIANGLE_SIZE;
        binary =
            size >= expected && (size == expected || !header[..5].eq_ignore_ascii_case(b"solid"));
    }
    let mut target = Mesh::new();
    let mut batch: Vec<[[f64; 3]; 3]> = Vec::new();
    if binary {
        file.seek(SeekFrom::Start(BINARY_HEADER_SIZE))?;
        let mut reader = BufReader::new(file);
        let mut rec = [0u8; BINARY_TRIANGLE_SIZE as usize];
        let step = (count / 100).max(1);
        for index in 0..count {
            reader
                .read_exact(&mut rec)
                .map_err(|_| invalid("STL binary facet data is truncated"))?;
            let f = |i: usize| {
                f32::from_le_bytes([rec[4 * i], rec[4 * i + 1], rec[4 * i + 2], rec[4 * i + 3]])
                    as f64
            };
            let mut tri = [[0.0; 3]; 3];
            for (k, base) in [3usize, 6, 9].into_iter().enumerate() {
                tri[k] =
                    scaled_point([f(base), f(base + 1), f(base + 2)], scale).map_err(invalid)?;
            }
            if non_degenerate(&tri) {
                batch.push(tri);
                if batch.len() >= BATCH_SIZE {
                    flush(&mut target, &mut batch);
                }
            }
            if index % step == 0 {
                tick(
                    &mut progress,
                    0.05 + 0.65 * (index + 1) as f64 / count.max(1) as f64,
                    "Reading file…",
                );
            }
        }
    } else {
        drop(file);
        ascii_triangles(path, scale, &mut progress, size, |tri| {
            if non_degenerate(&tri) {
                batch.push(tri);
                if batch.len() >= BATCH_SIZE {
                    flush(&mut target, &mut batch);
                }
            }
        })?;
    }
    flush(&mut target, &mut batch);
    if target.faces.is_empty() {
        return Err(invalid("STL file contains no usable triangles"));
    }
    if simplify != SimplifyMode::None {
        tick(&mut progress, 0.75, "Merging flat surfaces…");
        let _ = xtrude_core::fuse::simplify_mesh_opts(
            &mut target,
            0.0,
            simplify == SimplifyMode::Principal,
        );
    }
    if target.faces.len() > MAX_FUSE_LOOPS {
        tick(&mut progress, 1.0, "Done");
        return Ok(target);
    }
    let seed: Vec<DVec3> = target
        .face_ids()
        .iter()
        .flat_map(|&f| target.face_positions(f))
        .collect();
    let mut new_faces: HashSet<FaceId> = target.face_ids().into_iter().collect();
    tick(&mut progress, 0.85, "Joining triangles…");
    if simplify != SimplifyMode::None {
        orient_outward(&mut target, DEFAULT_SEED, None);
    } else {
        run_stitch(&mut target, &seed, Some(&mut new_faces), true, true);
        let only: Vec<FaceId> = target
            .face_ids()
            .into_iter()
            .filter(|f| new_faces.contains(f))
            .collect();
        orient_outward(&mut target, DEFAULT_SEED, Some(&only));
    }
    tick(&mut progress, 1.0, "Done");
    Ok(target)
}

/// Insert a prepared STL mesh into a scene: a big one as a reference group
/// named after the file, a small one into the editable loose mesh.
pub fn add_stl_mesh(scene: &mut Scene, path: &Path, target: Mesh) {
    if target.faces.len() > MAX_FUSE_LOOPS {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = scene.doc.add_mesh(target);
        scene.doc.groups.push(Group::new(id, Some(&name)));
    } else {
        let mesh = scene.mesh_mut();
        for f in target.face_ids() {
            mesh.add_face(&target.face_positions(f), &target.face_hole_positions(f));
        }
    }
    scene.version += 1;
}

/// Add binary or ASCII STL geometry to `scene`.
pub fn load_stl(
    scene: &mut Scene,
    path: &Path,
    progress: Option<Progress<'_>>,
    scale: f64,
    simplify: SimplifyMode,
) -> Result<(), StlError> {
    let target = parse_stl(path, progress, scale, simplify)?;
    add_stl_mesh(scene, path, target);
    Ok(())
}

fn normal(a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let n = (b - a).cross(c - a);
    let len = n.length();
    if len > 1e-12 {
        n / len
    } else {
        DVec3::ZERO
    }
}

/// Write the scene as a binary STL in `unit` (a key of [`STL_UNITS`];
/// millimetres by default).
pub fn save_stl(scene: &Scene, path: &Path, unit: &str) -> Result<(), StlError> {
    let per_unit = unit_scale(unit).ok_or_else(|| invalid(format!("unknown STL unit {unit:?}")))?;
    let k = 1.0 / per_unit;
    let tris: Vec<[DVec3; 3]> = iter_triangles(scene)
        .into_iter()
        .map(|t| t.map(|p| p * k))
        .collect();
    let mut out = Vec::with_capacity(84 + tris.len() * 50);
    let mut header = b"Xtrude CAD STL export".to_vec();
    header.resize(80, 0);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for [a, b, c] in tris {
        let n = normal(a, b, c);
        for v in [n, a, b, c] {
            for x in [v.x, v.y, v.z] {
                out.extend_from_slice(&(x as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}
