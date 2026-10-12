// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Native Xtrude CAD document format (`.igz`).
//!
//! Plain JSON, schema-versioned, compatible with IngeTrazo's `.igz`
//! (both open each other's documents). Layout:
//!
//! ```text
//! {
//!   "igz_format": 1,
//!   "app_version": "...",
//!   "scene": {
//!     "edges": [{"a": [x, y, z], "b": [x, y, z]}, ...],
//!     "faces": [{"vertices": [[x, y, z], ...]}],
//!     "groups": [...], "protos": [...], ...
//!   }
//! }
//! ```
//!
//! Upstream IngeTrazo wrote textured documents as ZIP containers (format
//! 2): the same JSON as `document.json` plus images. Xtrude CAD has no
//! textures: it still opens those containers and reads only
//! `document.json`; texture entries are dropped on load. Documents are
//! always written as plain JSON at format 1.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde_json::{json, Map, Value};
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::math::{DMat4, DVec3, MatExt};
use xtrude_core::mesh::{bump_curve_counter_past, EdgeFlags, Mesh};
use xtrude_core::topology::maximal_holes;
use xtrude_core::Axis;
use xtrude_model::definitions::split_divergent_copies;
use xtrude_model::dimension::Dimension;
use xtrude_model::dimension::DimensionStyle;
use xtrude_model::group::{reserve_group_names, Exploded, Group};
use xtrude_model::guide::Guide;
use xtrude_model::ids::MeshId;
use xtrude_model::layers::{Layer, DEFAULT_LAYER};
use xtrude_model::materials::Material;
use xtrude_model::saved_views::CameraHome;
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::{Doc, Scene};
use xtrude_model::section::SectionPlane;
use xtrude_model::style::Style;
use xtrude_model::textlabel::TextLabel;
use xtrude_model::units;

/// Bare JSON document: what this build writes.
pub const PLAIN_FORMAT: i64 = 1;
/// Newest format it reads: upstream's ZIP container.
pub const CURRENT_FORMAT: i64 = 2;
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";
const DOC_ENTRY: &str = "document.json";

/// A NaN / infinity standing in a JSON value while the payload is built
/// (`serde_json` cannot hold one): found and reported before writing.
const NON_FINITE: &str = "\u{0}non-finite\u{0}";

#[derive(Debug, thiserror::Error)]
pub enum IgzError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    /// The document holds a coordinate that is not a number; it was not
    /// written, so the file on disk keeps its last good version.
    #[error(
        "The model has a point with invalid coordinates (not a number), so it was not saved \
         and the file on disk keeps its last good version. Undo the last step and save again. \
         Where: {where_}"
    )]
    NonFinite { where_: String },
    #[error("Document format v{0} is newer than this build (v{CURRENT_FORMAT}).")]
    TooNew(i64),
    #[error("{0} is not an Xtrude CAD document (no document.json).")]
    NotADocument(String),
    #[error("{0} is not a valid Xtrude CAD document: {1}")]
    Corrupt(String, String),
}

fn num(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or_else(|| Value::String(NON_FINITE.into()), Value::Number)
}

fn point(p: DVec3) -> Value {
    Value::Array(vec![num(p.x), num(p.y), num(p.z)])
}

fn rgb(c: [f64; 3]) -> Value {
    Value::Array(c.iter().map(|&v| num(v)).collect())
}

/// A paint as the `.igz` stores it (`color`, `mat`, `opacity`).
pub fn paint_json(p: &Paint) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(c) = p.color {
        m.insert("color".into(), rgb(c));
    }
    if let Some(mat) = &p.mat {
        m.insert("mat".into(), Value::from(mat.clone()));
    }
    if let Some(o) = p.opacity {
        m.insert("opacity".into(), num(o));
    }
    m
}

fn rgb_of(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// A paint dict back (texture keys and anything unknown are dropped).
pub fn paint_from_json(raw: &Map<String, Value>) -> Paint {
    Paint {
        color: raw.get("color").and_then(rgb_of),
        mat: raw
            .get("mat")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        opacity: raw.get("opacity").and_then(Value::as_f64),
    }
}

fn face_json(mesh: &Mesh, f: xtrude_core::FaceId) -> Value {
    let face = mesh.face(f);
    let mut e = Map::new();
    e.insert(
        "vertices".into(),
        Value::Array(face.outer.iter().map(|&v| point(mesh.pos(v))).collect()),
    );
    if !face.holes.is_empty() {
        e.insert(
            "holes".into(),
            Value::Array(
                face.holes
                    .iter()
                    .map(|h| Value::Array(h.iter().map(|&v| point(mesh.pos(v))).collect()))
                    .collect(),
            ),
        );
    }
    let a = &face.attrs;
    if let Some(c) = a.paint.color {
        e.insert("color".into(), rgb(c));
    }
    if let Some(l) = &a.layer {
        e.insert("layer".into(), Value::from(l.clone()));
    }
    if let Some(o) = a.paint.opacity {
        e.insert("opacity".into(), num(o));
    }
    if a.hidden {
        e.insert("hidden".into(), Value::Bool(true));
    }
    if let Some(m) = a.paint.mat.as_ref().filter(|m| !m.is_empty()) {
        e.insert("mat".into(), Value::from(m.clone()));
    }
    match &a.back {
        Some(Back::Own(p)) => {
            e.insert("back".into(), Value::Object(paint_json(p)));
        }
        Some(Back::Mirror) => {
            e.insert("back".into(), Value::Bool(true));
        }
        None => {}
    }
    Value::Object(e)
}

fn edge_json(mesh: &Mesh, e: xtrude_core::EdgeId) -> Value {
    let ed = mesh.edge(e);
    let mut m = Map::new();
    m.insert("a".into(), point(mesh.pos(ed.v0)));
    m.insert("b".into(), point(mesh.pos(ed.v1)));
    if ed.soft {
        m.insert("soft".into(), Value::Bool(true));
    }
    if ed.hidden {
        m.insert("hidden".into(), Value::Bool(true));
    }
    if let Some(c) = ed.curve {
        m.insert("curve".into(), Value::from(c));
    }
    if let Some(l) = &ed.layer {
        m.insert("layer".into(), Value::from(l.clone()));
    }
    Value::Object(m)
}

fn mesh_json(mesh: &Mesh) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(
        "edges".into(),
        Value::Array(
            mesh.edge_ids()
                .into_iter()
                .map(|e| edge_json(mesh, e))
                .collect(),
        ),
    );
    m.insert(
        "faces".into(),
        Value::Array(
            mesh.face_ids()
                .into_iter()
                .map(|f| face_json(mesh, f))
                .collect(),
        ),
    );
    m
}

fn matrix_json(m: &DMat4) -> Value {
    Value::Array(m.data().iter().map(|&x| num(x)).collect())
}

struct Protos<'a> {
    doc: &'a Doc,
    index: HashMap<MeshId, usize>,
    list: Vec<Value>,
}

impl Protos<'_> {
    fn entry(&mut self, g: &Group) -> Value {
        let mut e = if let Some(xf) = &g.xform {
            let idx = match self.index.get(&g.mesh) {
                Some(&i) => i,
                None => {
                    let i = self.list.len();
                    self.index.insert(g.mesh, i);
                    self.list
                        .push(Value::Object(mesh_json(self.doc.mesh(g.mesh))));
                    i
                }
            };
            let mut m = Map::new();
            m.insert("proto".into(), Value::from(idx));
            m.insert("xform".into(), matrix_json(xf));
            m.insert("component".into(), Value::Bool(g.component));
            m
        } else {
            mesh_json(self.doc.mesh(g.mesh))
        };
        if !g.name.is_empty() {
            e.insert("name".into(), Value::from(g.name.clone()));
        }
        if let Some(axes) = &g.axes {
            e.insert("axes".into(), matrix_json(axes));
        }
        if let Some(l) = &g.layer {
            e.insert("layer".into(), Value::from(l.clone()));
        }
        if let Some(p) = g.material.as_ref().filter(|p| !p.is_empty()) {
            e.insert("material".into(), Value::Object(paint_json(p)));
        }
        if let Some(t) = g.text3d.as_ref().filter(|t| !t.is_empty()) {
            e.insert("text3d".into(), Value::Object(t.clone()));
        }
        if let Some(x) = g.ext.as_ref().filter(|x| !x.is_empty()) {
            e.insert("ext".into(), Value::Object(x.clone()));
        }
        if !g.uid.is_empty() {
            e.insert("uid".into(), Value::from(g.uid.clone()));
        }
        if g.hidden {
            e.insert("hidden".into(), Value::Bool(true));
        }
        if let Some(x) = &g.exploded {
            e.insert(
                "exploded".into(),
                json!({"factor": num(x.factor), "mode": x.mode}),
            );
        }
        if let Some(o) = g.explode_offset {
            e.insert("explode_offset".into(), point(o));
        }
        if !g.children.is_empty() {
            let kids: Vec<Value> = g.children.iter().map(|c| self.entry(c)).collect();
            e.insert("children".into(), Value::Array(kids));
        }
        Value::Object(e)
    }
}

/// The document as the JSON value written to disk.
pub fn scene_json(scene: &Scene) -> Value {
    let doc = &scene.doc;
    let mut payload = mesh_json(doc.mesh(MeshId::LOOSE));
    if !doc.groups.is_empty() {
        let mut protos = Protos {
            doc,
            index: HashMap::new(),
            list: Vec::new(),
        };
        let groups: Vec<Value> = doc.groups.iter().map(|g| protos.entry(g)).collect();
        payload.insert("groups".into(), Value::Array(groups));
        if !protos.list.is_empty() {
            payload.insert("protos".into(), Value::Array(protos.list));
        }
    }
    if doc.layers.len() > 1 || doc.layers.iter().any(|l| !l.visible || l.locked) {
        payload.insert(
            "layers".into(),
            Value::Array(doc.layers.iter().map(Layer::to_json).collect()),
        );
    }
    if !scene.saved_views.is_empty() {
        payload.insert(
            "saved_views".into(),
            Value::Array(scene.saved_views.iter().map(SavedView::to_json).collect()),
        );
    }
    if !doc.dimensions.is_empty() {
        let view = scene.doc_view();
        let dims: Vec<Value> = doc
            .dimensions
            .iter()
            .map(|d| {
                let mut m = Map::new();
                m.insert("a".into(), point(d.a(&view)));
                m.insert("b".into(), point(d.b(&view)));
                m.insert("offset".into(), point(d.offset));
                if let Some(ax) = d.axis {
                    m.insert("axis".into(), Value::from(ax.as_char().to_string()));
                }
                if let Some(l) = d.layer.as_ref().filter(|l| !l.is_empty()) {
                    m.insert("layer".into(), Value::from(l.clone()));
                }
                if let Some(t) = d.text.as_ref().filter(|t| !t.is_empty()) {
                    m.insert("text".into(), Value::from(t.clone()));
                }
                Value::Object(m)
            })
            .collect();
        payload.insert("dimensions".into(), Value::Array(dims));
    }
    if !doc.materials.is_empty() {
        payload.insert(
            "materials".into(),
            Value::Array(doc.materials.values().map(Material::to_json).collect()),
        );
    }
    payload.insert(
        "dimension_style".into(),
        Value::Object(scene.dimension_style.to_json()),
    );
    if let Some(cam) = &scene.camera_home {
        payload.insert("camera".into(), Value::Object(cam.to_json()));
    }
    payload.insert("units".into(), scene.units.to_json());
    if !doc.plugin_data.is_empty() {
        payload.insert("plugin_data".into(), Value::Object(doc.plugin_data.clone()));
    }
    if let Some(c) = scene.back_face_color {
        payload.insert("back_face_color".into(), rgb(c));
    }
    if !doc.section_planes.is_empty() {
        payload.insert(
            "section_planes".into(),
            Value::Array(
                doc.section_planes
                    .iter()
                    .map(SectionPlane::to_json)
                    .collect(),
            ),
        );
        let mut vis = Map::new();
        if !scene.show_section_planes {
            vis.insert("planes_hidden".into(), Value::Bool(true));
        }
        if !scene.show_section_cuts {
            vis.insert("cuts_hidden".into(), Value::Bool(true));
        }
        if !vis.is_empty() {
            payload.insert("section_view".into(), Value::Object(vis));
        }
    }
    let mut shown = Map::new();
    if scene.show_hidden_objects {
        shown.insert("objects".into(), Value::Bool(true));
    }
    if scene.show_hidden_geometry {
        shown.insert("geometry".into(), Value::Bool(true));
    }
    if !shown.is_empty() {
        payload.insert("hidden_view".into(), Value::Object(shown));
    }
    let style = scene.display_style.to_json();
    if style != Style::default().to_json() {
        payload.insert("display_style".into(), style);
    }
    if !doc.text_labels.is_empty() {
        payload.insert(
            "text_labels".into(),
            Value::Array(doc.text_labels.iter().map(TextLabel::to_json).collect()),
        );
    }
    if !doc.guides.is_empty() {
        payload.insert(
            "guides".into(),
            Value::Array(doc.guides.iter().map(Guide::to_json).collect()),
        );
    }
    json!({
        "igz_format": PLAIN_FORMAT,
        "app_version": xtrude_model::VERSION,
        "scene": Value::Object(payload),
    })
}

/// The first place in the payload holding a NaN or an infinity, as a path
/// (`scene › groups[0] › faces[3] …`).
fn non_finite_where(node: &Value, path: &str) -> Option<String> {
    let mut stack: Vec<(&Value, String)> = vec![(node, path.to_string())];
    while let Some((cur, here)) = stack.pop() {
        match cur {
            Value::String(s) if s == NON_FINITE => return Some(here),
            Value::Object(m) => {
                for (k, v) in m.iter().rev() {
                    stack.push((v, format!("{here} › {k}")));
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate().rev() {
                    stack.push((v, format!("{here}[{i}]")));
                }
            }
            _ => {}
        }
    }
    None
}

/// Write `scene` to `path` as plain JSON (atomically: a failure mid-write
/// leaves the existing file as it was). A coordinate that is not a number
/// refuses the save.
pub fn save_scene(scene: &Scene, path: &Path) -> Result<(), IgzError> {
    let data = scene_json(scene);
    if let Some(where_) = data.get("scene").and_then(|p| non_finite_where(p, "scene")) {
        return Err(IgzError::NonFinite { where_ });
    }
    let text = serde_json::to_string_pretty(&data)?;
    write_atomic(path, text.as_bytes())?;
    Ok(())
}

/// Write via a sibling temp file + rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    let tmp = path.with_file_name(name);
    let res = std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, path));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Replace the non-standard `NaN` / `Infinity` / `-Infinity` tokens
/// (outside strings) with a marker string, so a document written before
/// non-finite values were refused still parses. Returns whether any was
/// found.
fn neutralise_constants(text: &str) -> (String, bool) {
    if !(text.contains("NaN") || text.contains("Infinity")) {
        return (text.to_string(), false);
    }
    let mut out = String::with_capacity(text.len());
    let (mut in_str, mut escape, mut found) = (false, false, false);
    let mut skip_to = 0;
    for (i, ch) in text.char_indices() {
        if i < skip_to {
            continue;
        }
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            out.push(ch);
            continue;
        }
        let rest = &text[i..];
        if let Some(t) = ["-Infinity", "Infinity", "NaN"]
            .into_iter()
            .find(|t| rest.starts_with(t))
        {
            // the marker, escaped as JSON text
            out.push_str("\"\\u0000non-finite\\u0000\"");
            found = true;
            skip_to = i + t.len();
            continue;
        }
        if ch == '"' {
            in_str = true;
        }
        out.push(ch);
    }
    (out, found)
}

/// The document's JSON from either shape: plain JSON, or upstream's ZIP
/// container. Returns `(value, saw_non_finite)`.
fn read_document(path: &Path) -> Result<(Value, bool), IgzError> {
    let raw = std::fs::read(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text = if raw.starts_with(ZIP_MAGIC) {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw))
            .map_err(|e| IgzError::Corrupt(name.clone(), e.to_string()))?;
        let mut entry = archive
            .by_name(DOC_ENTRY)
            .map_err(|_| IgzError::NotADocument(name.clone()))?;
        let mut s = String::new();
        entry.read_to_string(&mut s)?;
        s
    } else {
        String::from_utf8(raw).map_err(|e| IgzError::Corrupt(name.clone(), e.to_string()))?
    };
    let (text, saw) = neutralise_constants(&text);
    let v: Value = serde_json::from_str(&text)?;
    Ok((v, saw))
}

fn coord(v: &Value) -> Option<DVec3> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    let p = DVec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?);
    if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
        Some(p)
    } else {
        None
    }
}

fn coords(v: Option<&Value>) -> Option<Vec<DVec3>> {
    v.and_then(Value::as_array)?.iter().map(coord).collect()
}

fn face_attrs_from_json(raw: &Value) -> Option<FaceAttrs> {
    let mut a = FaceAttrs::default();
    a.paint.color = raw.get("color").and_then(rgb_of);
    if let Some(l) = raw
        .get("layer")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        a.layer = Some(l.to_string());
    }
    a.paint.opacity = raw.get("opacity").and_then(Value::as_f64);
    a.hidden = raw.get("hidden").is_some_and(truthy);
    if let Some(m) = raw
        .get("mat")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        a.paint.mat = Some(m.to_string());
    }
    match raw.get("back") {
        Some(Value::Object(m)) => {
            let p = paint_from_json(m);
            if !p.is_empty() {
                a.back = Some(Back::Own(p));
            }
        }
        Some(Value::Bool(true)) => a.back = Some(Back::Mirror),
        _ => {}
    }
    if a.is_empty() {
        None
    } else {
        Some(a)
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

struct LoadState {
    dropped: usize,
}

/// One face record: outer ring, holes, attrs — `None` when a coordinate is
/// not a number (the face is left out).
type FaceRec = (Vec<DVec3>, Vec<Vec<DVec3>>, Option<FaceAttrs>);

fn read_faces(payload: &Value, st: &mut LoadState) -> Vec<FaceRec> {
    let mut out = Vec::new();
    for raw in payload
        .get("faces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let outer = coords(raw.get("vertices"));
        let holes: Option<Vec<Vec<DVec3>>> = match raw.get("holes").and_then(Value::as_array) {
            Some(hs) => hs.iter().map(|h| coords(Some(h))).collect(),
            None => Some(Vec::new()),
        };
        match (outer, holes) {
            (Some(o), Some(h)) => out.push((o, h, face_attrs_from_json(raw))),
            _ => st.dropped += 1,
        }
    }
    out
}

type EdgeRec = (DVec3, DVec3, EdgeFlags);

fn read_edges(payload: &Value, st: &mut LoadState) -> Vec<EdgeRec> {
    let mut out = Vec::new();
    for raw in payload
        .get("edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(a), Some(b)) = (raw.get("a").and_then(coord), raw.get("b").and_then(coord))
        else {
            st.dropped += 1;
            continue;
        };
        let flags = EdgeFlags {
            soft: raw.get("soft").is_some_and(truthy),
            hidden: raw.get("hidden").is_some_and(truthy),
            curve: raw.get("curve").and_then(Value::as_i64),
            layer: raw
                .get("layer")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        out.push((a, b, flags));
    }
    out
}

/// Rebuild a mesh from its JSON block. Small blocks take the plain
/// per-entity walk, big ones the bulk weld; the two paths create the edges
/// in different orders.
fn load_mesh(mesh: &mut Mesh, payload: &Value, st: &mut LoadState) {
    let edges = read_edges(payload, st);
    let faces = read_faces(payload, st);
    if edges.len() + faces.len() * 4 < 1024 {
        for (a, b, f) in &edges {
            let Ok(e) = mesh.add_edge(*a, *b) else {
                continue;
            };
            let ed = mesh.edge_mut(e);
            if f.soft {
                ed.soft = true;
            }
            if f.hidden {
                ed.hidden = true;
            }
            if f.layer.is_some() {
                ed.layer = f.layer.clone();
            }
            if let Some(c) = f.curve {
                ed.curve = Some(c);
                bump_curve_counter_past(c);
            }
        }
        for (outer, holes, attrs) in &faces {
            let f = mesh.add_face(outer, holes);
            if let Some(a) = attrs {
                mesh.face_mut(f).attrs.update(a);
            }
        }
        return;
    }
    let mut flat: Vec<DVec3> = Vec::new();
    for (a, b, _) in &edges {
        flat.push(*a);
        flat.push(*b);
    }
    let n_edge = flat.len();
    let mut ring_sizes = Vec::new();
    let mut ring_counts = Vec::new();
    let mut attrs = Vec::new();
    for (outer, holes, a) in &faces {
        let holes = if holes.len() > 1 {
            maximal_holes(holes)
        } else {
            holes.clone()
        };
        ring_counts.push(1 + holes.len());
        ring_sizes.push(outer.len());
        flat.extend(outer.iter().copied());
        for h in &holes {
            ring_sizes.push(h.len());
            flat.extend(h.iter().copied());
        }
        attrs.push(a.clone());
    }
    if flat.is_empty() {
        return;
    }
    let (vobjs, ids) = mesh.bulk_weld(&flat);
    if !edges.is_empty() {
        let ia: Vec<usize> = ids[..n_edge].iter().step_by(2).copied().collect();
        let ib: Vec<usize> = ids[..n_edge].iter().skip(1).step_by(2).copied().collect();
        let flags: Vec<EdgeFlags> = edges.iter().map(|e| e.2.clone()).collect();
        mesh.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
        if let Some(c) = flags.iter().filter_map(|f| f.curve).max() {
            bump_curve_counter_past(c);
        }
    }
    if !ring_counts.is_empty() {
        mesh.add_faces_welded(&vobjs, &ids[n_edge..], &ring_sizes, &ring_counts, &attrs);
    }
}

fn matrix_of(v: &Value) -> Option<DMat4> {
    let a = v.as_array()?;
    if a.len() != 16 {
        return None;
    }
    let mut vals = [0.0; 16];
    for (i, x) in a.iter().enumerate() {
        vals[i] = x.as_f64()?;
    }
    Some(DMat4::from_data(&vals))
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

/// Replace `scene`'s contents with what is stored at `path`.
pub fn load_into(
    scene: &mut Scene,
    path: &Path,
    mut progress: Option<Progress<'_>>,
) -> Result<(), IgzError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    tick(0.05, "Reading the document…");
    let (data, _saw) = read_document(path)?;
    let fmt = data.get("igz_format").and_then(Value::as_i64).unwrap_or(1);
    if fmt > CURRENT_FORMAT {
        return Err(IgzError::TooNew(fmt));
    }
    let empty = Value::Object(Map::new());
    let payload = data.get("scene").unwrap_or(&empty);
    let mut st = LoadState { dropped: 0 };

    // A fresh document state, so a document opened after another inherits
    // none of its dimensions, layers or section planes.
    scene.end_group_edit();
    scene.doc = Doc::default();
    scene.selection.clear();
    scene.saved_views.clear();
    scene.dimension_style = DimensionStyle::default();
    scene.back_face_color = None;
    scene.show_section_planes = true;
    scene.show_section_cuts = true;
    scene.show_hidden_objects = false;
    scene.show_hidden_geometry = false;

    tick(0.35, "Building geometry…");
    let mut loose = Mesh::new();
    load_mesh(&mut loose, payload, &mut st);
    scene
        .doc
        .meshes
        .insert(MeshId::LOOSE, std::sync::Arc::new(loose));
    if let Some(raw) = payload
        .get("layers")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        scene.doc.layers = raw.iter().map(Layer::from_json).collect();
        if !scene.doc.layers.iter().any(|l| l.name == DEFAULT_LAYER) {
            scene.doc.layers.insert(0, Layer::new(DEFAULT_LAYER));
        }
    }
    for raw in payload
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let m = Material::from_json(raw);
        if !m.name.is_empty() {
            scene.doc.materials.insert(m.name.clone(), m);
        }
    }
    scene.saved_views = payload
        .get("saved_views")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(SavedView::from_json).collect())
        .unwrap_or_default();
    let mut proto_ids = Vec::new();
    for raw in payload
        .get("protos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut m = Mesh::new();
        load_mesh(&mut m, raw, &mut st);
        proto_ids.push(scene.doc.add_mesh(m));
    }
    let mut names: Vec<String> = Vec::new();
    let raw_groups: Vec<&Value> = payload
        .get("groups")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let n_groups = raw_groups.len();
    for (i, raw) in raw_groups.into_iter().enumerate() {
        if i % 50 == 0 {
            tick(
                0.6 + 0.35 * i as f64 / n_groups.max(1) as f64,
                "Placing groups…",
            );
        }
        let g = group_from(scene, raw, &proto_ids, &mut names, &mut st, 0);
        scene.doc.groups.push(g);
    }
    // Copies of one prototype whose contents differ (edited apart before
    // the editor shared contents) become separate components.
    split_divergent_copies(&mut scene.doc.meshes, &mut scene.doc.groups);
    tick(0.96, "Finishing…");
    if !names.is_empty() {
        reserve_group_names(names.iter().map(String::as_str));
    }

    for raw in payload
        .get("dimensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(a), Some(b), Some(off)) = (
            raw.get("a").and_then(coord),
            raw.get("b").and_then(coord),
            raw.get("offset").and_then(coord),
        ) else {
            st.dropped += 1;
            continue;
        };
        let mut dim = Dimension::new(a, b, off).with_axis(
            raw.get("axis")
                .and_then(Value::as_str)
                .and_then(|s| s.chars().next())
                .and_then(Axis::from_char),
        );
        dim.layer = raw
            .get("layer")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        dim.text = raw
            .get("text")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        scene.bind_dimension(&mut dim);
        scene.doc.dimensions.push(dim);
    }
    // A document from before the ends could be chosen drew oblique ticks:
    // it keeps them. Only a NEW document is born with arrows.
    scene.dimension_style.ends = "tick".into();
    if let Some(style) = payload.get("dimension_style").and_then(Value::as_object) {
        scene.dimension_style.merge_json(style);
    }
    scene.camera_home = payload
        .get("camera")
        .and_then(Value::as_object)
        .map(CameraHome::from_json);
    scene.units = if payload.get("units").is_some() {
        units::model_units_of_payload(payload)
    } else {
        units::legacy_document_units()
    };
    scene.bind_units();
    scene.doc.plugin_data = payload
        .get("plugin_data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(c) = payload.get("back_face_color").and_then(rgb_of) {
        scene.back_face_color = Some(c);
    }
    scene.display_style = payload
        .get("display_style")
        .filter(|v| v.is_object())
        .map(Style::from_json)
        .unwrap_or_default();
    if let Some(raw) = payload.get("section_planes").and_then(Value::as_array) {
        scene.doc.section_planes = raw.iter().filter_map(SectionPlane::from_json).collect();
        let mut seen_active = false;
        for sp in scene.doc.section_planes.iter_mut() {
            if sp.active && seen_active {
                sp.active = false;
            }
            seen_active |= sp.active;
        }
    }
    if let Some(vis) = payload.get("section_view").and_then(Value::as_object) {
        scene.show_section_planes = !vis.get("planes_hidden").is_some_and(truthy);
        scene.show_section_cuts = !vis.get("cuts_hidden").is_some_and(truthy);
    }
    if let Some(shown) = payload.get("hidden_view").and_then(Value::as_object) {
        scene.show_hidden_objects = shown.get("objects").is_some_and(truthy);
        scene.show_hidden_geometry = shown.get("geometry").is_some_and(truthy);
    }
    for raw in payload
        .get("text_labels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        scene.doc.text_labels.push(TextLabel::from_json(raw));
    }
    for raw in payload
        .get("guides")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        // A guide at (nan, nan, nan) stalled every tool: left out.
        let finite = ["point", "direction", "origin"]
            .iter()
            .all(|k| match raw.get(*k) {
                None | Some(Value::Null) => true,
                Some(v) => coord(v).is_some(),
            });
        match Guide::from_json(raw).filter(|_| finite) {
            Some(g) => scene.doc.guides.push(g),
            None => st.dropped += 1,
        }
    }
    scene.load_repairs = st.dropped;
    scene.version += 1;
    Ok(())
}

fn group_from(
    scene: &mut Scene,
    raw: &Value,
    protos: &[MeshId],
    names: &mut Vec<String>,
    st: &mut LoadState,
    depth: usize,
) -> Group {
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if let Some(n) = name {
        names.push(n.to_string());
    }
    let xform = raw.get("xform").and_then(matrix_of);
    let proto = raw
        .get("proto")
        .and_then(Value::as_u64)
        .and_then(|i| protos.get(i as usize).copied());
    let mut group = match (xform, proto) {
        (Some(xf), Some(pid)) => {
            let mut g = Group::new(pid, name);
            g.xform = Some(xf);
            g
        }
        _ => {
            let mut m = Mesh::new();
            load_mesh(&mut m, raw, st);
            let id = scene.doc.add_mesh(m);
            Group::new(id, name)
        }
    };
    if let Some(axes) = raw.get("axes").and_then(matrix_of) {
        group.axes = Some(axes);
    }
    if let Some(l) = raw
        .get("layer")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        group.layer = Some(l.to_string());
    }
    if let Some(t) = raw.get("text3d").and_then(Value::as_object) {
        group.text3d = Some(t.clone());
    }
    if let Some(x) = raw.get("ext").and_then(Value::as_object) {
        group.ext = Some(x.clone());
    }
    match raw.get("uid") {
        Some(Value::String(s)) if !s.is_empty() => group.uid = s.clone(),
        Some(v) if truthy(v) => group.uid = v.to_string(),
        _ => {}
    }
    if raw.get("hidden").is_some_and(truthy) {
        group.hidden = true;
    }
    if let Some(m) = raw.get("material").and_then(Value::as_object) {
        let p = paint_from_json(m);
        group.material = if p.is_empty() { None } else { Some(p) };
    }
    if let Some(x) = raw.get("exploded").and_then(Value::as_object) {
        group.exploded = Some(Exploded {
            factor: x.get("factor").and_then(Value::as_f64).unwrap_or(0.0),
            mode: x
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    if let Some(o) = raw.get("explode_offset").and_then(coord) {
        group.explode_offset = Some(o);
    }
    let kids_raw: Vec<&Value> = raw
        .get("children")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if depth < 32 {
        let kids: Vec<Group> = kids_raw
            .iter()
            .map(|c| group_from(scene, c, protos, names, st, depth + 1))
            .collect();
        if !kids.is_empty() {
            group.adopt(kids);
        }
    }
    if let Some(c) = raw.get("component") {
        group.component = truthy(c);
    } else if kids_raw
        .iter()
        .any(|c| c.is_object() && c.get("xform").is_none())
    {
        // Saved before the key existed: a container holding a
        // CLASSIC group can only be Make Group's. A group, then.
        group.component = false;
    }
    group
}
