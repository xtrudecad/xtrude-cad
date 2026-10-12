// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Structured parse errors (`openskp/errors.py`).
//!
//! [`SkpParseError`] carries *where* a parse failed: the stage, the
//! top-level record (and how many there are), the TLV tag, the byte offset
//! and the definition. The underlying error is kept as the `source`, the
//! way Python keeps it as `__cause__`.

use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;

use crate::tlv::Tag;

/// Which pipeline stage was running (`SkpParseError.stage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Header,
    ZipExtract,
    TlvWalk,
    BuildScene,
    LegacyWalk,
    LegacyDefs,
}

impl Stage {
    /// The Python spelling (`"zip_extract"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Header => "header",
            Stage::ZipExtract => "zip_extract",
            Stage::TlvWalk => "tlv_walk",
            Stage::BuildScene => "build_scene",
            Stage::LegacyWalk => "legacy_walk",
            Stage::LegacyDefs => "legacy_defs",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parse failure with structured location context (`SkpParseError`).
#[derive(Debug, Default)]
pub struct SkpParseError {
    pub message: String,
    pub stage: Option<Stage>,
    pub record_index: Option<usize>,
    pub total_records: Option<usize>,
    pub tag: Option<Tag>,
    pub offset: Option<usize>,
    pub definition_id: Option<String>,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl SkpParseError {
    pub fn new(message: impl Into<String>, stage: Stage) -> Self {
        SkpParseError {
            message: message.into(),
            stage: Some(stage),
            ..Default::default()
        }
    }

    /// The `raise … from exc` half: keeps `cause` as the error source.
    pub fn caused_by(mut self, cause: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(cause));
        self
    }

    pub fn at_record(mut self, index: usize, total: usize) -> Self {
        self.record_index = Some(index);
        self.total_records = Some(total);
        self
    }

    pub fn with_tag(mut self, tag: Tag) -> Self {
        self.tag = Some(tag);
        self
    }

    pub fn at_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn in_definition(mut self, id: impl fmt::Display) -> Self {
        self.definition_id = Some(id.to_string());
        self
    }
}

/// `SkpParseError.__str__`: the message, then `stage=…`, `record=N/M`,
/// `tag=…`, `offset=0x…`, `definition_id=…`, joined by `" | "`.
impl fmt::Display for SkpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(stage) = self.stage {
            write!(f, " | stage={stage}")?;
        }
        if let (Some(i), Some(n)) = (self.record_index, self.total_records) {
            write!(f, " | record={i}/{n}")?;
        }
        if let Some(tag) = self.tag {
            write!(f, " | tag={tag}")?;
        }
        if let Some(offset) = self.offset {
            write!(f, " | offset=0x{offset:X}")?;
        }
        if let Some(id) = &self.definition_id {
            write!(f, " | definition_id={id}")?;
        }
        Ok(())
    }
}

impl StdError for SkpParseError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn StdError + 'static))
    }
}

/// Everything [`crate::SkpFile`] can fail with. Python raises
/// `FileNotFoundError` and `ValueError` from `SkpFile.open` and
/// `SkpParseError` from `parse`; here they are one enum.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("File not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("Expected a .skp file, got: {0}")]
    NotSkp(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] SkpParseError),
}
