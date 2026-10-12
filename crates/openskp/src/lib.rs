// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A SketchUp (`.skp`) reader derived from the read path of
//! [OpenSKP](https://github.com/iamahsanmehmood/openskp) (MIT), at commit
//! `291700bc`, `packages/python/openskp`.
//!
//! Only reading is implemented; OpenSKP's writer, editor and exporters
//! are not. See `UPSTREAM.md` for the module map.
//!
//! ```no_run
//! let model = openskp::SkpFile::open("model.skp")?.parse()?;
//! println!("{} definitions", model.definitions.len());
//! # Ok::<(), openskp::Error>(())
//! ```

pub mod container;
pub mod errors;
pub mod legacy;
pub mod model;
pub mod parsed;
pub mod py;
pub mod tlv;
pub mod vff;
mod xml;

use std::path::{Path, PathBuf};

pub use container::Container;
pub use errors::{Error, SkpParseError, Stage};
pub use indexmap::IndexMap;
pub use model::*;
pub use parsed::Parsed;

/// An `.skp` file to parse (`openskp/model.py SkpFile`).
#[derive(Debug, Clone)]
pub struct SkpFile {
    pub path: PathBuf,
}

impl SkpFile {
    /// `SkpFile.open`: the file must exist and end in `.skp` (any case).
    pub fn open(path: impl AsRef<Path>) -> Result<SkpFile, Error> {
        let path = path.as_ref();
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if !path.exists() {
            return Err(Error::NotFound(path));
        }
        let is_skp = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("skp"));
        if !is_skp {
            let suffix = path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            return Err(Error::NotSkp(suffix));
        }
        Ok(SkpFile { path })
    }

    /// Read the file and open its container (`full_parse` steps 1–2).
    pub fn read_container(&self) -> Result<Container, Error> {
        Ok(container::open(std::fs::read(&self.path)?)?)
    }

    /// `SkpFile.parse`: the whole pipeline, classic or VFF.
    pub fn parse(&self) -> Result<SkpModel, Error> {
        Ok(SkpModel::from_parsed(full_parse(&self.path)?))
    }
}

/// `_core.full_parse`: parse the `.skp` at `path` into the intermediate
/// [`Parsed`] form both walkers produce.
pub fn full_parse(path: impl AsRef<Path>) -> Result<Parsed, Error> {
    let data = std::fs::read(path)?;
    Ok(full_parse_bytes(data)?)
}

/// [`full_parse`] on the file's bytes.
pub fn full_parse_bytes(data: Vec<u8>) -> Result<Parsed, SkpParseError> {
    match container::open(data)? {
        Container::Legacy(data) => legacy::full_parse_legacy(&data),
        Container::Vff(zf) => vff::full_parse(zf),
    }
}
