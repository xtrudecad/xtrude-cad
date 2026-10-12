// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! TLV records of `model.dat` (`openskp/_core.py`, "TLV Parsing").
//!
//! VFF (2021+) `model.dat` is a stream of Type-Length-Value records: a
//! 2-byte tag, a little-endian `u32` length, then the payload. Most tags
//! carry raw leaf payloads; container tags hold nested records.
//!
//! `openskp/parser.py` is an older copy of the same functions that the
//! real pipeline does not use; its tests run against this module.

use std::fmt;

use crate::errors::{SkpParseError, Stage};

/// A record tag, in file byte order. Displays as the Python hex string
/// (`F401`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tag(pub [u8; 2]);

impl Tag {
    pub const fn new(b0: u8, b1: u8) -> Tag {
        Tag([b0, b1])
    }

    const fn index(self) -> usize {
        ((self.0[0] as usize) << 8) | self.0[1] as usize
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X}{:02X}", self.0[0], self.0[1])
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tag({self})")
    }
}

/// A set of tags as a 65536-bit table, so the per-record container check
/// on the hot path is one lookup (the Python set membership test).
pub struct TagSet([u64; 1024]);

impl TagSet {
    pub const fn new(tags: &[Tag]) -> TagSet {
        let mut bits = [0u64; 1024];
        let mut i = 0;
        while i < tags.len() {
            let k = tags[i].index();
            bits[k / 64] |= 1 << (k % 64);
            i += 1;
        }
        TagSet(bits)
    }

    pub const fn contains(&self, tag: Tag) -> bool {
        let k = tag.index();
        self.0[k / 64] & (1 << (k % 64)) != 0
    }
}

pub const F401: Tag = Tag::new(0xF4, 0x01);
pub const F901: Tag = Tag::new(0xF9, 0x01);
pub const T7017: Tag = Tag::new(0x70, 0x17);
pub const T7117: Tag = Tag::new(0x71, 0x17);
/// One component/group definition (`_DEFINITION_TAG`).
pub const DEFINITION: Tag = Tag::new(0x7C, 0x15);

/// `CONTAINER_TAGS`: every tag the TLV parser recurses into.
pub static CONTAINER_TAGS: TagSet = TagSet::new(&[
    F401,
    Tag::new(0xF7, 0x01),
    Tag::new(0xD4, 0x30),
    Tag::new(0xD5, 0x30),
    Tag::new(0xC8, 0x32),
    DEFINITION,
    Tag::new(0x88, 0x13),
    Tag::new(0x89, 0x13),
    Tag::new(0x8A, 0x13),
    Tag::new(0x8B, 0x13),
    Tag::new(0x8C, 0x13),
    Tag::new(0x8D, 0x13),
    Tag::new(0x4C, 0x1D),
    Tag::new(0x64, 0x19),
    F901,
    T7017,
    T7117,
    Tag::new(0xD0, 0x07),
    Tag::new(0xC4, 0x09),
    Tag::new(0x94, 0x11),
    Tag::new(0x95, 0x11),
    Tag::new(0x0F, 0x01),
    Tag::new(0x38, 0x4A),
    Tag::new(0xB8, 0x0B),
    Tag::new(0x97, 0x13),
    Tag::new(0x2C, 0x4C),
    Tag::new(0xAC, 0x0D),
    Tag::new(0xAE, 0x0D),
    Tag::new(0xF6, 0x01),
    Tag::new(0xF8, 0x01),
    Tag::new(0x98, 0x3A),
    Tag::new(0x99, 0x3A),
    Tag::new(0x8C, 0x3C),
    Tag::new(0x8D, 0x3C),
    // Image-entity placement: an Image placed in the model wraps a
    // standard 6419 instance node inside 9013 -> 401F.
    Tag::new(0x90, 0x13),
    Tag::new(0x40, 0x1F),
]);

/// Deepest container nesting [`parse_tlv_recursive`] follows. Python's
/// recursion limit (1000 frames) stopped a hostile file there with a
/// `RecursionError`; this keeps the same bound as a parse error instead of
/// a stack overflow.
pub const MAX_TLV_DEPTH: usize = 960;

/// Size of a record header: 2-byte tag + `u32` length.
pub const HEADER_LEN: usize = 6;

/// `read_u32`: little-endian `u32` at `offset`, `None` past the end.
pub fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let b = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

/// `read_f64`: little-endian `f64` at `offset`, `None` past the end.
pub fn read_f64(data: &[u8], offset: usize) -> Option<f64> {
    let b = data.get(offset..offset.checked_add(8)?)?;
    Some(f64::from_le_bytes(b.try_into().ok()?))
}

/// `parse_var_int`: a little-endian unsigned integer of `length` bytes
/// (at most 8; Python's integers are unbounded, real files use ≤ 8).
pub fn parse_var_int(data: &[u8], offset: usize, length: usize) -> Option<u64> {
    if length > 8 {
        return None;
    }
    let b = data.get(offset..offset.checked_add(length)?)?;
    Some(
        b.iter()
            .enumerate()
            .fold(0u64, |v, (i, &byte)| v | u64::from(byte) << (8 * i)),
    )
}

/// One parsed TLV record (`_TlvNode`). A container's `payload` is empty:
/// its content lives in `children`. Payloads borrow from the buffer, so
/// Python's lazy slicing is unnecessary.
#[derive(Debug, Clone)]
pub struct TlvNode<'a> {
    pub offset: usize,
    pub tag: Tag,
    pub size: usize,
    pub children: Vec<TlvNode<'a>>,
    pub payload: &'a [u8],
}

/// A record header without its subtree (`_flat_headers` tuple).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub tag: Tag,
    pub offset: usize,
    pub size: usize,
}

/// The header at `pos` if a whole record fits before `end`. The guard is
/// `pos + 6 <= end` (openskp's boundary fix: a header filling exactly the
/// last 6 bytes is a real record).
fn header_at(data: &[u8], pos: usize, end: usize) -> Option<Header> {
    if pos + HEADER_LEN > end {
        return None;
    }
    let size = read_u32(data, pos + 2)? as usize;
    if pos + HEADER_LEN + size > end {
        return None;
    }
    Some(Header {
        tag: Tag([data[pos], data[pos + 1]]),
        offset: pos,
        size,
    })
}

/// `parse_tlv_recursive`: the records in `data[start..end]`, recursing
/// into the tags in `containers`. A truncated trailing record ends the
/// list, as in Python. `end` is clamped to the buffer.
pub fn parse_tlv_recursive<'a>(
    data: &'a [u8],
    start: usize,
    end: usize,
    containers: &TagSet,
) -> Result<Vec<TlvNode<'a>>, SkpParseError> {
    parse_level(data, start, end.min(data.len()), containers, 0)
}

fn parse_level<'a>(
    data: &'a [u8],
    start: usize,
    end: usize,
    containers: &TagSet,
    depth: usize,
) -> Result<Vec<TlvNode<'a>>, SkpParseError> {
    if depth > MAX_TLV_DEPTH {
        return Err(
            SkpParseError::new("TLV records nested too deeply", Stage::TlvWalk).at_offset(start),
        );
    }
    let mut elements = Vec::new();
    let mut pos = start;
    while let Some(h) = header_at(data, pos, end) {
        let body = pos + HEADER_LEN..pos + HEADER_LEN + h.size;
        let children = if containers.contains(h.tag) && h.size > 0 {
            parse_level(data, body.start, body.end, containers, depth + 1)?
        } else {
            Vec::new()
        };
        let payload = if children.is_empty() {
            &data[body]
        } else {
            &[]
        };
        elements.push(TlvNode {
            offset: pos,
            tag: h.tag,
            size: h.size,
            children,
            payload,
        });
        pos += HEADER_LEN + h.size;
    }
    Ok(elements)
}

/// `_flat_headers`: the direct-child headers in `data[start..end]`,
/// without recursing.
pub fn flat_headers(data: &[u8], start: usize, end: usize) -> Vec<Header> {
    let end = end.min(data.len());
    let mut headers = Vec::new();
    let mut pos = start;
    while let Some(h) = header_at(data, pos, end) {
        headers.push(h);
        pos += HEADER_LEN + h.size;
    }
    headers
}

/// `_unwrap_definitions_container`: `F901` wraps the definition list in
/// `7017` then `7117`; returns the `7C15` headers inside, or `None` when
/// the file has another shape.
fn unwrap_definitions_container(data: &[u8], h: Header) -> Option<Vec<Header>> {
    let mut level = flat_headers(data, h.offset + HEADER_LEN, h.offset + HEADER_LEN + h.size);
    for expected in [T7017, T7117] {
        let c = level.iter().find(|c| c.tag == expected)?;
        level = flat_headers(data, c.offset + HEADER_LEN, c.offset + HEADER_LEN + c.size);
    }
    let defs: Vec<Header> = level.into_iter().filter(|c| c.tag == DEFINITION).collect();
    (!defs.is_empty()).then_some(defs)
}

/// `iter_top_level_lazy`: each top-level record, fully parsed, one at a
/// time, as `(index, total, node)`. A lone `F401` wrapper is unwrapped,
/// and an `F901` definition list yields one item per definition, so only
/// one subtree is alive at a time.
pub fn iter_top_level<'a, 's>(
    data: &'a [u8],
    start: usize,
    end: usize,
    containers: &'s TagSet,
) -> TopLevel<'a, 's> {
    let mut headers = flat_headers(data, start, end);
    if let [only] = headers[..] {
        if only.tag == F401 {
            headers = flat_headers(
                data,
                only.offset + HEADER_LEN,
                only.offset + HEADER_LEN + only.size,
            );
        }
    }
    let mut expanded = Vec::with_capacity(headers.len());
    for h in headers {
        match (h.tag == F901)
            .then(|| unwrap_definitions_container(data, h))
            .flatten()
        {
            Some(defs) => expanded.extend(defs),
            None => expanded.push(h),
        }
    }
    TopLevel {
        data,
        containers,
        total: expanded.len(),
        headers: expanded.into_iter().enumerate(),
    }
}

/// Iterator returned by [`iter_top_level`].
pub struct TopLevel<'a, 's> {
    data: &'a [u8],
    containers: &'s TagSet,
    headers: std::iter::Enumerate<std::vec::IntoIter<Header>>,
    total: usize,
}

impl TopLevel<'_, '_> {
    /// Number of top-level records (`total`).
    pub fn total(&self) -> usize {
        self.total
    }
}

impl<'a> Iterator for TopLevel<'a, '_> {
    type Item = Result<(usize, usize, TlvNode<'a>), SkpParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        for (index, h) in self.headers.by_ref() {
            let end = h.offset + HEADER_LEN + h.size;
            match parse_tlv_recursive(self.data, h.offset, end, self.containers) {
                Ok(nodes) => {
                    if let Some(node) = nodes.into_iter().next() {
                        return Some(Ok((index, self.total, node)));
                    }
                }
                Err(e) => return Some(Err(e.at_record(index, self.total).with_tag(h.tag))),
            }
        }
        None
    }
}
