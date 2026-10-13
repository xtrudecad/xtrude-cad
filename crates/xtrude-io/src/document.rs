// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Xtrude CAD document (`.xtrude`, `application/x-xtrudecad`).
//!
//! Gzip-compressed JSON, versioned by `xtrude_format`:
//!
//! ```text
//! {
//!   "xtrude_format": 2,
//!   "app_version": "...",
//!   "scene": { ... }
//! }
//! ```
//!
//! The `scene` object is the schema in [`crate::schema`]. A reader
//! refuses a document whose `xtrude_format` is newer than [`FORMAT`]; it
//! also reads the JSON uncompressed (a document unpacked by hand).
//!
//! Format 2 added the print materials (the document's and the groups'
//! own): a format-1 reader would drop them on save, so it refuses the
//! document instead. A format-1 document opens with every group in the
//! default material, PLA.
//! IngeTrazo's `.igz` documents are read by [`crate::igz`]; this build
//! never writes one.

use std::io::{Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use xtrude_model::scene::Scene;

pub use crate::schema::Progress;
use crate::schema::{non_finite_where, read_payload, scene_payload, GeomBlocks};

/// The document's file extension, without the dot.
pub const EXTENSION: &str = "xtrude";
/// The document's MIME type.
pub const MIME: &str = "application/x-xtrudecad";
/// The format this build writes, and the newest it reads.
pub const FORMAT: i64 = 2;
const GZIP_MAGIC: &[u8] = b"\x1f\x8b";

#[derive(Debug, thiserror::Error)]
pub enum DocError {
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
    #[error("Document format v{0} is newer than this build (v{FORMAT}).")]
    TooNew(i64),
    #[error("{0} is not an Xtrude CAD document.")]
    NotADocument(String),
    #[error("{0} is not a valid Xtrude CAD document: {1}")]
    Corrupt(String, String),
}

/// Whether `path` has the document's extension (any case).
pub fn is_document_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// The document as the JSON value it stores.
pub fn scene_json(scene: &Scene) -> Value {
    json!({
        "xtrude_format": FORMAT,
        "app_version": xtrude_model::VERSION,
        "scene": Value::Object(scene_payload(scene)),
    })
}

/// The document as the bytes of a file. A coordinate that is not a number
/// refuses it.
pub fn to_bytes(scene: &Scene) -> Result<Vec<u8>, DocError> {
    let data = scene_json(scene);
    if let Some(where_) = data.get("scene").and_then(|p| non_finite_where(p, "scene")) {
        return Err(DocError::NonFinite { where_ });
    }
    let text = serde_json::to_vec(&data)?;
    let mut z = GzEncoder::new(Vec::new(), Compression::default());
    z.write_all(&text)?;
    Ok(z.finish()?)
}

/// Write `scene` to `path` (atomically: a failure mid-write leaves the
/// existing file as it was). A coordinate that is not a number refuses
/// the save.
pub fn save_scene(scene: &Scene, path: &Path) -> Result<(), DocError> {
    let data = to_bytes(scene)?;
    write_atomic(path, &data)?;
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

/// Replace `scene`'s contents with the document stored at `path`.
pub fn load_into(
    scene: &mut Scene,
    path: &Path,
    progress: Option<Progress<'_>>,
) -> Result<(), DocError> {
    let raw = std::fs::read(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    load_bytes(scene, &raw, &name, progress)
}

/// Replace `scene`'s contents with the document in `raw`, the bytes of a
/// file called `name` (for the messages).
pub fn load_bytes(
    scene: &mut Scene,
    raw: &[u8],
    name: &str,
    mut progress: Option<Progress<'_>>,
) -> Result<(), DocError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    tick(0.05, "Reading the document…");
    let corrupt = |e: &dyn std::fmt::Display| DocError::Corrupt(name.to_string(), e.to_string());
    let text = if raw.starts_with(GZIP_MAGIC) {
        let mut s = Vec::new();
        GzDecoder::new(raw)
            .read_to_end(&mut s)
            .map_err(|e| corrupt(&e))?;
        s
    } else {
        raw.to_vec()
    };
    let data: Value = serde_json::from_slice(&text).map_err(|e| corrupt(&e))?;
    let Some(fmt) = data.get("xtrude_format").and_then(Value::as_i64) else {
        return Err(DocError::NotADocument(name.to_string()));
    };
    if fmt > FORMAT {
        return Err(DocError::TooNew(fmt));
    }
    let Some(payload) = data.get("scene").filter(|p| p.is_object()) else {
        return Err(corrupt(&"no scene"));
    };
    read_payload(scene, payload, &GeomBlocks::new(), &mut tick);
    Ok(())
}
