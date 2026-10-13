// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Copy and paste between Xtrude CAD windows: the model
//! clipboard's wire format.
//!
//! The payload is a small zip: `clip.json` (reference point and the loose
//! edges) plus `scene.xtrude`, a throwaway document holding the copied faces
//! and groups — written by the regular document writer, so materials,
//! component definitions and group axes travel exactly as in a file. The
//! system-clipboard plumbing (MIME type [`MIME`], lazy rendering) lives in
//! the application.

use std::io::{Read, Write};

use serde_json::{json, Value};
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::math::DMat4;
use xtrude_core::DVec3;
use xtrude_model::group::{Group, MeshTable};
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

use crate::document::{self, DocError};

/// The system-clipboard MIME type of a model clipboard.
pub const MIME: &str = "application/x-xtrude-clipboard";
/// 2: the scene travels as an Xtrude CAD document (`scene.xtrude`).
const FORMAT: i64 = 2;
const SCENE_ENTRY: &str = "scene.xtrude";

/// A model clipboard (`viewport.clipboard`).
#[derive(Clone, Debug, Default)]
pub struct ClipData {
    /// `(outer, holes, attrs)` of each copied loose face.
    pub faces: Vec<(Vec<DVec3>, Vec<Vec<DVec3>>, FaceAttrs)>,
    /// `(a, b, soft, curve)` of each copied loose edge.
    pub edges: Vec<(DVec3, DVec3, bool, Option<i64>)>,
    /// The copied groups; their meshes are in `meshes`.
    pub groups: Vec<Group>,
    pub meshes: MeshTable,
    /// The reference point the paste is placed by.
    pub reference: DVec3,
}

fn xyz(p: DVec3) -> Value {
    json!([p.x, p.y, p.z])
}

fn point(v: &Value) -> Option<DVec3> {
    let a = v.as_array()?;
    Some(DVec3::new(
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ))
}

/// A clipboard as bytes for the system clipboard.
pub fn encode(clip: &ClipData) -> Result<Vec<u8>, DocError> {
    let mut scene = Scene::new();
    for (lp, holes, attrs) in &clip.faces {
        let m = scene.mesh_mut();
        let f = m.add_face(lp, holes);
        if !attrs.is_empty() {
            m.face_mut(f).attrs = attrs.clone();
        }
    }
    for g in &clip.groups {
        for (pg, _) in xtrude_model::group::iter_placements(g, None) {
            if let Some(m) = clip.meshes.get(&pg.mesh) {
                scene.doc.meshes.insert(pg.mesh, m.clone());
            }
        }
        scene.doc.groups.push(g.clone());
    }
    let head = json!({
        "format": FORMAT,
        "ref": xyz(clip.reference),
        "edges": clip.edges.iter().map(|(a, b, soft, curve)| json!([xyz(*a), xyz(*b), soft, curve])).collect::<Vec<_>>(),
    });
    let doc = document::to_bytes(&scene)?;
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let werr = |e: zip::result::ZipError| DocError::Io(std::io::Error::other(e));
        z.start_file("clip.json", opts).map_err(werr)?;
        z.write_all(head.to_string().as_bytes())?;
        z.start_file(SCENE_ENTRY, opts).map_err(werr)?;
        z.write_all(&doc)?;
        z.finish().map_err(werr)?;
    }
    Ok(out.into_inner())
}

/// Bytes from the system clipboard as a clipboard, or `None` when they are
/// not one this version can read.
pub fn decode(data: &[u8]) -> Option<ClipData> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(data)).ok()?;
    let mut head = String::new();
    z.by_name("clip.json")
        .ok()?
        .read_to_string(&mut head)
        .ok()?;
    let mut doc = Vec::new();
    z.by_name(SCENE_ENTRY).ok()?.read_to_end(&mut doc).ok()?;
    let head: Value = serde_json::from_str(&head).ok()?;
    if head.get("format").and_then(Value::as_i64) != Some(FORMAT) {
        return None;
    }
    let mut scene = Scene::new();
    document::load_bytes(&mut scene, &doc, SCENE_ENTRY, None).ok()?;
    let m = scene.doc.mesh(MeshId::LOOSE);
    let faces = m
        .face_ids()
        .into_iter()
        .map(|f| {
            (
                m.face_positions(f),
                m.face_hole_positions(f),
                m.face(f).attrs.clone(),
            )
        })
        .collect();
    let edges = head
        .get("edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let a = e.as_array()?;
            Some((
                point(a.first()?)?,
                point(a.get(1)?)?,
                a.get(2)?.as_bool().unwrap_or(false),
                a.get(3).and_then(Value::as_i64),
            ))
        })
        .collect();
    let mut groups = scene.doc.groups.clone();
    // Pasted copies of a classic group share one definition until edited.
    for g in groups.iter_mut() {
        if g.xform.is_none() {
            g.xform = Some(DMat4::IDENTITY);
            g.component = false;
        }
    }
    let mut meshes = scene.doc.meshes.clone();
    meshes.remove(&MeshId::LOOSE);
    Some(ClipData {
        faces,
        edges,
        groups,
        meshes,
        reference: point(head.get("ref")?)?,
    })
}
