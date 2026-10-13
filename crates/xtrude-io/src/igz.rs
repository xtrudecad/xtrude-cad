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
//! texture entries are dropped.

use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};
use xtrude_model::scene::Scene;

pub use crate::schema::Progress;
use crate::schema::{neutralise_constants, read_payload};

/// The newest format this build reads: the ZIP container.
pub const CURRENT_FORMAT: i64 = 2;
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

/// The document's JSON from either shape: plain JSON, or the ZIP
/// container.
fn read_document(path: &Path) -> Result<Value, IgzError> {
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
    // a document written before non-finite values were refused may hold
    // NaN / Infinity tokens
    let (text, _) = neutralise_constants(&text);
    Ok(serde_json::from_str(&text)?)
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
    let data = read_document(path)?;
    let fmt = data.get("igz_format").and_then(Value::as_i64).unwrap_or(1);
    if fmt > CURRENT_FORMAT {
        return Err(IgzError::TooNew(fmt));
    }
    let empty = Value::Object(Map::new());
    let payload = data.get("scene").unwrap_or(&empty);
    read_payload(scene, payload, &mut tick);
    Ok(())
}
