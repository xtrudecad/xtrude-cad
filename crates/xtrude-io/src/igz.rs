// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! IngeTrazo documents (`.igz`), read only: Xtrude CAD opens and imports
//! them, and saves in its own format ([`crate::document`]).
//!
//! Plain JSON, versioned by `igz_format`:
//!
//! ```text
//! {
//!   "igz_format": 1,
//!   "app_version": "...",
//!   "scene": { ... }
//! }
//! ```
//!
//! The `scene` object is the schema in [`crate::schema`]. Format 2 is a
//! ZIP container of the same JSON as `document.json` plus texture images:
//! Xtrude CAD has no textures, so it reads only `document.json` and the
//! texture entries are dropped. Format 3, a large document, is the ZIP
//! container with each big mesh as a binary block (`geom/<n>.bin`, read in
//! [`geom`]) instead of its `edges`/`faces` lists; it reopens with the same
//! vertices as the same mesh stored as JSON.
//!
//! Dropped on import, having nothing to map onto in Xtrude CAD's document:
//! `kept_components` (component definitions with no placed copy: the
//! document keeps a definition only through its copies), a group's
//! `locked` (groups are not locked: the group opens editable), a section
//! plane's own `fill`, and the units beyond the length and its precision
//! (`area`, `volume`, `angle`, their precisions, and `decimal`: areas and
//! volumes follow the length unit).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};
use xtrude_model::scene::Scene;

pub use crate::schema::Progress;
use crate::schema::{neutralise_constants, read_payload, GeomBlocks, MAX_GROUP_DEPTH};

mod geom;

/// The newest format this build reads: the ZIP container with binary
/// geometry blocks.
pub const CURRENT_FORMAT: i64 = 3;
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";
const DOC_ENTRY: &str = "document.json";

#[derive(Debug, thiserror::Error)]
pub enum IgzError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("IngeTrazo document format v{0} is newer than this build reads (v{CURRENT_FORMAT}).")]
    TooNew(i64),
    #[error("{0} is not an IngeTrazo document (no document.json).")]
    NotADocument(String),
    #[error("{0} is not a valid IngeTrazo document: {1}")]
    Corrupt(String, String),
}

/// Whether `path` has the `.igz` extension (any case).
pub fn is_igz_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("igz"))
}

/// A document read: its JSON, and the binary geometry members of a ZIP
/// container by name (none for plain JSON).
struct RawDocument {
    data: Value,
    members: HashMap<String, Vec<u8>>,
}

/// The file name of `path`, for the messages.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The document's JSON from either shape: plain JSON, or the ZIP
/// container.
fn read_document(path: &Path) -> Result<RawDocument, IgzError> {
    let raw = std::fs::read(path)?;
    let name = file_name(path);
    let mut members = HashMap::new();
    let text = if raw.starts_with(ZIP_MAGIC) {
        let corrupt = |e: zip::result::ZipError| IgzError::Corrupt(name.clone(), e.to_string());
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).map_err(corrupt)?;
        let mut s = String::new();
        archive
            .by_name(DOC_ENTRY)
            .map_err(|_| IgzError::NotADocument(name.clone()))?
            .read_to_string(&mut s)?;
        let names: Vec<String> = archive
            .file_names()
            .filter(|n| n.starts_with(geom::GEOM_PREFIX))
            .map(str::to_string)
            .collect();
        for member in names {
            let mut bytes = Vec::new();
            archive
                .by_name(&member)
                .map_err(corrupt)?
                .read_to_end(&mut bytes)?;
            members.insert(member, bytes);
        }
        s
    } else {
        String::from_utf8(raw).map_err(|e| IgzError::Corrupt(name.clone(), e.to_string()))?
    };
    // a document written before non-finite values were refused may hold
    // NaN / Infinity tokens
    let (text, _) = neutralise_constants(&text);
    Ok(RawDocument {
        data: serde_json::from_str(&text)?,
        members,
    })
}

/// The entries of the list `key` of `v` (none when it is not a list).
fn list<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// Every binary geometry block the payload's meshes use, read and checked:
/// the loose mesh, the prototypes and the groups at every depth read.
fn geometry_blocks(
    payload: &Value,
    members: &HashMap<String, Vec<u8>>,
    name: &str,
) -> Result<GeomBlocks, IgzError> {
    let mut meshes: Vec<&Value> = vec![payload];
    meshes.extend(list(payload, "protos"));
    let mut groups: Vec<(&Value, usize)> = list(payload, "groups")
        .into_iter()
        .map(|g| (g, 0))
        .collect();
    while let Some((g, depth)) = groups.pop() {
        meshes.push(g);
        if depth < MAX_GROUP_DEPTH {
            groups.extend(list(g, "children").into_iter().map(|c| (c, depth + 1)));
        }
    }
    let mut blocks = GeomBlocks::new();
    let mut metas: HashMap<&str, &Value> = HashMap::new();
    for meta in meshes.into_iter().filter_map(|m| m.get("geom")) {
        let damaged = |why: String| IgzError::Corrupt(name.to_string(), why);
        let member = meta.get("member").and_then(Value::as_str);
        if let Some(seen) = member.and_then(|m| metas.get(m)) {
            // one block used twice is read once; two layouts of one are not
            if *seen != meta {
                return Err(damaged("damaged geometry block (shared member)".into()));
            }
            continue;
        }
        let records = geom::decode(meta, members).map_err(damaged)?;
        if let Some(m) = member {
            metas.insert(m, meta);
            blocks.insert(m.to_string(), records);
        }
    }
    Ok(blocks)
}

/// Replace `scene`'s contents with the IngeTrazo document at `path`.
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
    let doc = read_document(path)?;
    let fmt = doc
        .data
        .get("igz_format")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    if fmt > CURRENT_FORMAT {
        return Err(IgzError::TooNew(fmt));
    }
    let empty = Value::Object(Map::new());
    let payload = doc.data.get("scene").unwrap_or(&empty);
    let geom = geometry_blocks(payload, &doc.members, &file_name(path))?;
    read_payload(scene, payload, &geom, &mut tick);
    Ok(())
}
