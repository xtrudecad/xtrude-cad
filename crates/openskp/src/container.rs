// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The `.skp` container (`openskp/_core.py full_parse` steps 1–2,
//! `_validate_zip_entry_size`, `_read_meta_units`; `openskp/vff.py
//! validate_header`).
//!
//! Every `.skp` starts with [`MAGIC`]. A classic (pre-2021) file is an MFC
//! archive (see [`crate::legacy`]); a VFF (2021+) file is a UTF-16 version
//! string followed by a ZIP holding `model.dat`, `meta/meta.dat`, the
//! material XMLs and the thumbnail.

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::errors::{SkpParseError, Stage};
use crate::legacy;

/// The four bytes every `.skp` starts with (`_VFF_MAGIC`).
pub const MAGIC: [u8; 4] = [0xFF, 0xFE, 0xFF, 0x0E];

/// How much of the file's start is searched for the header fields.
const HEADER_LEN: usize = 512;
/// How far `full_parse` looks for the ZIP's first local header.
const ZIP_SEARCH_LEN: usize = 4096;
const ZIP_LOCAL_HEADER: &[u8] = b"PK\x03\x04";

/// `_MAX_UNCOMPRESSED_ENTRY_BYTES`: no entry may claim more than 16 GB.
pub const MAX_UNCOMPRESSED_ENTRY_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// `_MAX_COMPRESSION_RATIO`: a larger ratio is treated as a ZIP bomb.
pub const MAX_COMPRESSION_RATIO: f64 = 1000.0;
/// `_RATIO_CHECK_THRESHOLD_BYTES`: smaller entries skip the ratio check.
pub const RATIO_CHECK_THRESHOLD_BYTES: u64 = 1024 * 1024;

/// `meta/meta.dat` record tags: the wrapper and the unit-system string.
const META_WRAPPER_TAG: [u8; 2] = [0x64, 0x00];
const META_UNITS_TAG: [u8; 2] = [0x6D, 0x00];

/// `vff.validate_header`: does `data` start with the `.skp` magic?
pub fn validate_header(data: &[u8]) -> bool {
    data.starts_with(&MAGIC)
}

/// The version string of a VFF header (`{24.0.484}`), or `"unknown"`.
/// It is UTF-16LE text after the second `FF FE FF` marker.
pub fn read_version(header: &[u8]) -> String {
    let header = &header[..header.len().min(HEADER_LEN)];
    let Some(marker) = find(header, b"\xFF\xFE\xFF", 4) else {
        return "unknown".into();
    };
    let units = header
        .get(marker + 4..)
        .unwrap_or_default()
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]));
    // errors='ignore': unpaired surrogates are dropped.
    let text: String = char::decode_utf16(units).filter_map(Result::ok).collect();
    match (text.find('{'), text.find('}')) {
        (Some(open), Some(close)) if close > open => text[open..=close].to_string(),
        _ => "unknown".into(),
    }
}

/// Offset of the embedded ZIP. Python searches the first 512 bytes, then
/// the first 4096; the first match in 4096 bytes is the same answer.
pub fn find_zip_offset(data: &[u8]) -> Option<usize> {
    find(&data[..data.len().min(ZIP_SEARCH_LEN)], ZIP_LOCAL_HEADER, 0)
}

/// `_validate_zip_entry_size`: reject an entry whose declared size is
/// implausible before anything decompresses it.
pub fn validate_zip_entry_size(
    name: &str,
    declared: u64,
    compressed: u64,
) -> Result<(), SkpParseError> {
    if declared == 0 {
        return Ok(());
    }
    if declared > MAX_UNCOMPRESSED_ENTRY_BYTES {
        return Err(SkpParseError::new(
            format!(
                "ZIP entry '{name}' declares {declared} bytes uncompressed, \
                 exceeding the {MAX_UNCOMPRESSED_ENTRY_BYTES}-byte safety ceiling"
            ),
            Stage::ZipExtract,
        ));
    }
    if declared >= RATIO_CHECK_THRESHOLD_BYTES
        && (compressed == 0 || declared as f64 / compressed as f64 > MAX_COMPRESSION_RATIO)
    {
        return Err(SkpParseError::new(
            format!(
                "ZIP entry '{name}' declares an implausible compression ratio \
                 ({declared} bytes from {compressed} bytes compressed) - \
                 likely a decompression bomb"
            ),
            Stage::ZipExtract,
        ));
    }
    Ok(())
}

/// `_read_meta_units`: the unit-system string (`"Millimeter"`) from
/// `meta/meta.dat`, a flat TLV list inside one `0x6400` wrapper record.
pub fn read_meta_units(meta: &[u8]) -> Option<String> {
    let mut pos = 0;
    while pos + 6 <= meta.len() {
        let tag = [meta[pos], meta[pos + 1]];
        let size = crate::tlv::read_u32(meta, pos + 2)? as usize;
        let body = meta.get(pos + 6..pos + 6 + size)?;
        if tag == META_WRAPPER_TAG {
            return read_meta_units(body);
        }
        if tag == META_UNITS_TAG {
            return Some(String::from_utf8_lossy(body).into_owned());
        }
        pos += 6 + size;
    }
    None
}

/// The ZIP of a VFF `.skp`, with the size guard on every read.
pub struct VffArchive {
    /// The header's version string, `"unknown"` when absent.
    pub version: String,
    zip: ZipArchive<Cursor<Vec<u8>>>,
}

impl VffArchive {
    /// Entry names in archive order (`ZipFile.namelist()`).
    pub fn names(&self) -> Vec<String> {
        self.zip
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// `_validate_zip_entry_size` then `zf.read(name)`.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, SkpParseError> {
        let zip_err = |e: zip::result::ZipError| {
            SkpParseError::new(
                format!("Cannot read ZIP entry '{name}': {e}"),
                Stage::ZipExtract,
            )
            .caused_by(e)
        };
        let mut entry = self.zip.by_name(name).map_err(zip_err)?;
        validate_zip_entry_size(name, entry.size(), entry.compressed_size())?;
        let mut out = Vec::with_capacity(entry.size().min(RATIO_CHECK_THRESHOLD_BYTES) as usize);
        entry.read_to_end(&mut out).map_err(|e| {
            SkpParseError::new(
                format!("Cannot read ZIP entry '{name}': {e}"),
                Stage::ZipExtract,
            )
            .caused_by(e)
        })?;
        Ok(out)
    }

    /// The model's unit system from `meta/meta.dat` (VFF only). A missing
    /// or unreadable entry gives `None`, as in Python.
    pub fn units(&mut self) -> Option<String> {
        if !self.contains("meta/meta.dat") {
            return None;
        }
        self.read("meta/meta.dat")
            .ok()
            .and_then(|m| read_meta_units(&m))
    }
}

/// An opened `.skp`: which walker its content needs.
pub enum Container {
    /// A classic MFC archive: the whole file, for the legacy walker.
    Legacy(Vec<u8>),
    Vff(VffArchive),
}

/// `full_parse` steps 1–2: check the magic, route classic files to the
/// legacy walker, read the version and open the embedded ZIP.
pub fn open(mut data: Vec<u8>) -> Result<Container, SkpParseError> {
    let header = &data[..data.len().min(HEADER_LEN)];
    if !validate_header(header) {
        return Err(SkpParseError::new(
            "Not a valid SketchUp file (bad header magic)",
            Stage::Header,
        ));
    }
    if legacy::is_legacy(header) {
        return Ok(Container::Legacy(data));
    }
    let version = read_version(header);
    let pk = find_zip_offset(&data)
        .ok_or_else(|| SkpParseError::new("No ZIP container found", Stage::ZipExtract))?;
    data.drain(..pk);
    let zip = ZipArchive::new(Cursor::new(data)).map_err(|e| {
        SkpParseError::new(
            format!("Cannot open the ZIP container: {e}"),
            Stage::ZipExtract,
        )
        .caused_by(e)
    })?;
    Ok(Container::Vff(VffArchive { version, zip }))
}

/// `bytes.find(needle, from)`.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}
