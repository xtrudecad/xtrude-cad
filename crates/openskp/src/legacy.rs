// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Classic (SketchUp 2013–2020) `.skp` files (`openskp/legacy.py`).
//!
//! A classic file is not a ZIP: after the UTF-16 header records the body
//! is one MFC `CArchive` object stream with a single 1-based store map.
//! This module walks that stream and adapts the result to the same
//! [`Parsed`] shape the VFF walker produces.
//!
//! Python decodes each record into a dict (`{'k': 'edge', …}`) and keeps
//! the store map as `slot → ('class'|'obj', name, value)`. Here a record
//! is an [`Obj`] and the store map an ordered map of [`Slot`]s; records
//! are shared through `Rc` as Python shares the dicts.
//!
//! **Dimensions anchored inside groups.** A `CDimensionLinear` follows
//! each connection reference with an entity reference and two lists of
//! references (the instance path of the anchored entity). The pinned
//! upstream reader skips a fixed 42/82 bytes there, which is only right
//! while the lists are empty; this reader reads the lists
//! (iamahsanmehmood/openskp#384, the fix the Python code applied at run
//! time).
//!
//! Errors keep Python's exception classes apart, because the walk treats
//! them differently: an entity list recovers only from a
//! `LegacyParseError`, the slot-base retry also from `struct.error`,
//! `IndexError` and `UnicodeDecodeError`.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::container::MAGIC;
use crate::errors::{SkpParseError, Stage};
use crate::model::{AttrDicts, AttrValue, Point, Texture};
use crate::parsed::{
    root_definition, ArcParams, GeometryBuilder, Parsed, RawConstructionLine, RawDefinition,
    RawDimension, RawFace, RawInstance, RawMaterial, RawPage, RawSectionPlane, RawText,
};
use crate::py;

/// `is_legacy`: true when `data` (the file's first bytes) is a classic
/// MFC-container `.skp`.
pub fn is_legacy(data: &[u8]) -> bool {
    if !data.starts_with(&MAGIC) {
        return false;
    }
    if contains(&data[..data.len().min(0x100)], b"PK\x03\x04") {
        return false;
    }
    contains(&data[..data.len().min(0x200)], b"CVersionMap")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `bytes.find(needle, start, end)`, `end` clamped to the buffer.
fn find(hay: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let end = end.min(hay.len());
    if start > end {
        return None;
    }
    hay[start..end]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

/// A failed walk, by the Python exception it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyError {
    /// `LegacyParseError`: a structural error the walker recognised.
    Parse(String),
    /// `struct.error` / `IndexError`: a read past the end of the data.
    Struct(String),
    /// `UnicodeDecodeError`: a class name that is not ASCII.
    Unicode(String),
    /// Anything else Python lets escape (`RecursionError` on a hostile
    /// nesting depth, `TypeError` on an impossible reference): no retry
    /// catches it.
    Fatal(String),
}

impl LegacyError {
    fn is_retryable(&self) -> bool {
        !matches!(self, LegacyError::Fatal(_))
    }
}

impl fmt::Display for LegacyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegacyError::Parse(m)
            | LegacyError::Struct(m)
            | LegacyError::Unicode(m)
            | LegacyError::Fatal(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LegacyError {}

type R<T> = Result<T, LegacyError>;

fn parse_err<T>(msg: impl Into<String>) -> R<T> {
    Err(LegacyError::Parse(msg.into()))
}

/// The UTF-16 string record marker.
pub const STR_MARKER: &[u8; 3] = b"\xff\xfe\xff";

/// How deeply records may nest. Python stops a hostile file with a
/// `RecursionError` at about 1000 frames, three or more per record.
const MAX_DEPTH: usize = 250;

// ── byte cursor ─────────────────────────────────────────────────────────

/// `_R`: a byte cursor. Like Python's, `raw` past the end returns a
/// short slice and still advances; fixed-size reads fail there.
#[derive(Debug, Clone)]
pub struct Cursor<'d> {
    pub data: &'d [u8],
    pub pos: usize,
}

impl<'d> Cursor<'d> {
    pub fn new(data: &'d [u8], pos: usize) -> Self {
        Cursor { data, pos }
    }

    fn fixed(&mut self, n: usize) -> R<&'d [u8]> {
        let b = self
            .pos
            .checked_add(n)
            .and_then(|end| self.data.get(self.pos..end))
            .ok_or_else(|| {
                LegacyError::Struct(format!(
                    "unpack_from requires a buffer of at least {} bytes (actual buffer size is {})",
                    self.pos.saturating_add(n),
                    self.data.len()
                ))
            })?;
        self.pos += n;
        Ok(b)
    }

    pub fn u8(&mut self) -> R<u8> {
        let v = *self
            .data
            .get(self.pos)
            .ok_or_else(|| LegacyError::Struct("index out of range".into()))?;
        self.pos += 1;
        Ok(v)
    }

    pub fn u16(&mut self) -> R<u16> {
        let b = self.fixed(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> R<u32> {
        let b = self.fixed(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn f64(&mut self) -> R<f64> {
        let b = self.fixed(8)?;
        Ok(f64_le(b))
    }

    pub fn f64s<const N: usize>(&mut self) -> R<[f64; N]> {
        let b = self.fixed(8 * N)?;
        Ok(std::array::from_fn(|i| f64_le(&b[8 * i..])))
    }

    pub fn raw(&mut self, n: usize) -> &'d [u8] {
        let start = self.pos.min(self.data.len());
        let end = self.pos.saturating_add(n).min(self.data.len());
        self.pos = self.pos.saturating_add(n);
        &self.data[start..end]
    }

    pub fn peek(&self, n: usize) -> &'d [u8] {
        let start = self.pos.min(self.data.len());
        let end = self.pos.saturating_add(n).min(self.data.len());
        &self.data[start..end]
    }

    pub fn peek_u16(&self) -> R<u16> {
        u16_at(self.data, self.pos)
    }

    /// An MFC string record: `FF FE FF`, a u8 length (escalating to u16,
    /// then u32), then that many UTF-16LE code units.
    pub fn utf16(&mut self) -> R<String> {
        if self.peek(3) != STR_MARKER {
            return parse_err(format!("expected a string record {}", self.ctx()));
        }
        self.pos += 3;
        let mut n = u32::from(self.u8()?);
        if n == 0xFF {
            n = u32::from(self.u16()?);
            if n == 0xFFFF {
                n = self.u32()?;
            }
        }
        let b = self.raw(2 * n as usize);
        Ok(py::utf16le(b, true))
    }

    /// `ctx()`: the position and the bytes around it, for messages.
    pub fn ctx(&self) -> String {
        let p = self.pos.min(self.data.len());
        let hex = |b: &[u8]| {
            b.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!(
            "@{:#x}: …{} | {}…",
            self.pos,
            hex(&self.data[p.saturating_sub(16)..p]),
            hex(self.peek(32))
        )
    }
}

fn f64_le(b: &[u8]) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    f64::from_le_bytes(a)
}

fn u16_at(data: &[u8], at: usize) -> R<u16> {
    match at.checked_add(2).and_then(|e| data.get(at..e)) {
        Some(b) => Ok(u16::from_le_bytes([b[0], b[1]])),
        None => Err(LegacyError::Struct(format!(
            "unpack_from requires a buffer of at least {} bytes (actual buffer size is {})",
            at.saturating_add(2),
            data.len()
        ))),
    }
}

fn u32_at(data: &[u8], at: usize) -> R<u32> {
    match at.checked_add(4).and_then(|e| data.get(at..e)) {
        Some(b) => Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        None => Err(LegacyError::Struct(format!(
            "unpack_from requires a buffer of at least {} bytes (actual buffer size is {})",
            at.saturating_add(4),
            data.len()
        ))),
    }
}

fn f64_at(data: &[u8], at: usize) -> R<f64> {
    match at.checked_add(8).and_then(|e| data.get(at..e)) {
        Some(b) => Ok(f64_le(b)),
        None => Err(LegacyError::Struct(format!(
            "unpack_from requires a buffer of at least {} bytes (actual buffer size is {})",
            at.saturating_add(8),
            data.len()
        ))),
    }
}

fn vec3_at(data: &[u8], at: usize) -> R<Point> {
    Ok([
        f64_at(data, at)?,
        f64_at(data, at + 8)?,
        f64_at(data, at + 16)?,
    ])
}

/// `bytes.hex().upper()` of a GUID.
fn hex_upper(b: &[u8]) -> String {
    py::hex_upper(b)
}

// ── records ─────────────────────────────────────────────────────────────

/// `_drawbase`: material, display flags and layer of a drawing element.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DrawBase {
    pub mat: u16,
    pub hidden: u8,
    pub soft: u8,
    pub smooth: u8,
    pub layer: u16,
}

/// The textured-material block shared by `CMaterial` and `CLayer`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureBlock {
    pub rgba: [u8; 4],
    pub opacity: f64,
    pub use_opacity: u8,
    pub tex_dib: Option<i64>,
    pub tex_w: f64,
    pub tex_h: f64,
    pub tex_file: String,
    pub colorized: bool,
}

/// One entity-list item: `(slot, class name, value)`.
pub type Ent = (Option<i64>, Option<Rc<str>>, Option<Rc<Obj>>);

/// One attribute-dictionary child: `(name, value)`.
pub type AttrChild = (Option<Rc<str>>, Option<Rc<Obj>>);

/// A connection's anchored entity and its two path lists.
pub type ConnectionPaths = (Option<i64>, Vec<Option<i64>>, Vec<Option<i64>>);

/// A decoded record (Python's `{'k': …}` dicts).
#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Vertex {
        xyz: Point,
    },
    Edge {
        db: DrawBase,
        curve: Option<i64>,
        curve_value: Option<Rc<Obj>>,
        v1: Option<i64>,
        v2: Option<i64>,
    },
    Curve {
        n: u32,
    },
    ArcCurve(ArcParams),
    EdgeUse {
        edge: Option<i64>,
        sense: u8,
    },
    Loop {
        uses: Vec<Option<Rc<Obj>>>,
    },
    Face {
        db: DrawBase,
        plane: [f64; 4],
        loops: Vec<Option<Rc<Obj>>>,
        back_mat: u16,
        attrs: Option<Rc<Obj>>,
    },
    Attrs {
        children: Vec<AttrChild>,
    },
    Dict {
        name: String,
        entries: IndexMap<String, AttrValue>,
    },
    Layer {
        name: String,
        hidden: u8,
        rgba: [u8; 4],
    },
    Material {
        name: String,
        rgba: [u8; 4],
        opacity: f64,
        use_opacity: u8,
        texture: Option<TextureBlock>,
    },
    Dib {
        subtype: u32,
        data: Vec<u8>,
    },
    Ftc {
        front: [f64; 9],
        back: [f64; 9],
        front_projected: bool,
        back_projected: bool,
    },
    Camera,
    Thumbnail,
    Image {
        db: DrawBase,
        def: Option<i64>,
        xform: [f64; 12],
        guid: String,
    },
    Relationship,
    Cline {
        db: DrawBase,
        point: Point,
        direction: Point,
        start: Option<Point>,
        end: Option<Point>,
    },
    Cpoint {
        db: DrawBase,
        pos: Point,
    },
    SectionPlane {
        db: DrawBase,
        plane: Vec<f64>,
        name: String,
        label: String,
    },
    Font,
    Dimension {
        db: DrawBase,
        text: String,
        connect: (Option<i64>, Option<i64>),
        offset: f64,
        a: Option<Point>,
        b: Option<Point>,
    },
    Text {
        db: DrawBase,
        text: String,
        p: Option<Point>,
        lp: Option<Point>,
    },
    Definition {
        name: String,
        guid: String,
        ents: Vec<Ent>,
        faces_camera: bool,
        shadows_face_sun: bool,
    },
    Instance {
        db: DrawBase,
        def: Option<i64>,
        xf: [f64; 13],
        name: String,
        guid: String,
        attrs: Option<Rc<Obj>>,
    },
}

impl Obj {
    /// `value.get('name')`: the records that carry a name.
    fn name(&self) -> Option<&str> {
        match self {
            Obj::Layer { name, .. }
            | Obj::Material { name, .. }
            | Obj::Dict { name, .. }
            | Obj::Definition { name, .. }
            | Obj::Instance { name, .. }
            | Obj::SectionPlane { name, .. } => Some(name),
            _ => None,
        }
    }

    /// `value.get('rgba')`.
    fn rgba(&self) -> Option<[u8; 4]> {
        match self {
            Obj::Layer { rgba, .. } | Obj::Material { rgba, .. } => Some(*rgba),
            _ => None,
        }
    }
}

/// One store-map entry: `('class', name, schema)` or `('obj', name, value)`.
#[derive(Debug, Clone, PartialEq)]
pub enum Slot {
    Class {
        name: Rc<str>,
        schema: Option<u16>,
    },
    Obj {
        name: Rc<str>,
        value: Option<Rc<Obj>>,
    },
}

// ── the archive ─────────────────────────────────────────────────────────

/// `_Archive`: MFC `CArchive` store-map bookkeeping.
pub struct Archive<'d> {
    pub data: &'d [u8],
    /// SketchUp major version (16, 17, …).
    pub ver: i64,
    /// `CEntity` schema 5 (2017+) stores persistent-id masks.
    pub has_pid: bool,
    pub slots: IndexMap<i64, Slot>,
    pub class_slot: HashMap<String, i64>,
    pub next_slot: i64,
    /// Slots below this are the unwalked region before the model.
    pub walk_base: i64,
    current_loop: Option<i64>,
    in_entity_list: bool,
    /// Burned store-map indices (see `register_burn`): `(file band
    /// start, width)` per event, their total, and the walker slot right
    /// after the last annotation record.
    burns: Vec<(i64, i64)>,
    cum_delta: i64,
    annot_watermark: Option<i64>,
    burn_stack: Vec<i64>,
    /// The construction-line trailing block length, calibrated on the
    /// file's first guide line.
    cline_tail: Option<usize>,
    depth: usize,
}

/// What `read_object` returns: `(slot, class name, value)`.
type Read = (Option<i64>, Option<Rc<str>>, Option<Rc<Obj>>);

impl<'d> Archive<'d> {
    pub fn new(data: &'d [u8], ver: i64) -> Self {
        Archive {
            data,
            ver,
            has_pid: ver >= 17,
            slots: IndexMap::new(),
            class_slot: HashMap::new(),
            next_slot: 0,
            walk_base: 0,
            current_loop: None,
            in_entity_list: false,
            burns: Vec::new(),
            cum_delta: 0,
            annot_watermark: None,
            burn_stack: Vec::new(),
            cline_tail: None,
            depth: 0,
        }
    }

    fn alloc(&mut self, entry: Slot) -> i64 {
        let s = self.next_slot;
        self.slots.insert(s, entry);
        self.next_slot += 1;
        s
    }

    /// `read_object`: a null, a new class (and its first object), an
    /// object of a known class, or a back-reference.
    pub fn read_object(&mut self, r: &mut Cursor<'d>, expect: Option<&str>) -> R<Read> {
        let tag = r.u16()?;
        if tag == 0 {
            return Ok((None, None, None));
        }
        if tag == 0x7FFF {
            // Big-tag escape.
            let big = r.u32()?;
            if big & 0x8000_0000 != 0 {
                return self.new_of_class(r, i64::from(big & 0x7FFF_FFFF), expect);
            }
            return self.backref(i64::from(big), r);
        }
        if tag == 0xFFFF {
            let schema = r.u16()?;
            let namelen = r.u16()?;
            if namelen > 40 {
                return parse_err(format!("implausible class name length {}", r.ctx()));
            }
            let raw = r.raw(namelen as usize);
            if !raw.is_ascii() {
                return Err(LegacyError::Unicode(
                    "'ascii' codec can't decode the class name".into(),
                ));
            }
            let name: Rc<str> = Rc::from(std::str::from_utf8(raw).unwrap_or_default());
            let slot = self.alloc(Slot::Class {
                name: name.clone(),
                schema: Some(schema),
            });
            self.class_slot.insert(name.to_string(), slot);
            return self.new_obj(r, name);
        }
        if tag & 0x8000 != 0 {
            return self.new_of_class(r, i64::from(tag & 0x7FFF), expect);
        }
        self.backref(i64::from(tag), r)
    }

    fn new_of_class(&mut self, r: &mut Cursor<'d>, cslot: i64, expect: Option<&str>) -> R<Read> {
        let name = match self.slots.get(&cslot) {
            Some(Slot::Class { name, .. }) => name.clone(),
            Some(Slot::Obj { name, .. }) => {
                return parse_err(format!(
                    "class-ref to non-class slot {cslot} ({name}) {}",
                    r.ctx()
                ))
            }
            None => {
                let Some(expect) = expect else {
                    return parse_err(format!("class-ref to unknown slot {cslot} {}", r.ctx()));
                };
                // A class defined in the unwalked pre-model region: learn
                // it from context.
                let name: Rc<str> = Rc::from(expect);
                self.slots.insert(
                    cslot,
                    Slot::Class {
                        name: name.clone(),
                        schema: None,
                    },
                );
                self.class_slot.insert(expect.to_string(), cslot);
                name
            }
        };
        self.new_obj(r, name)
    }

    fn new_obj(&mut self, r: &mut Cursor<'d>, name: Rc<str>) -> R<Read> {
        self.in_entity_list = false;
        let slot = self.alloc(Slot::Obj {
            name: name.clone(),
            value: None,
        });
        if !is_known_class(&name) {
            return parse_err(format!("no reader for class {name} {}", r.ctx()));
        }
        if self.depth >= MAX_DEPTH {
            return Err(LegacyError::Fatal(
                "maximum recursion depth exceeded".into(),
            ));
        }
        self.depth += 1;
        let value = read_class(self, r, &name);
        self.depth -= 1;
        let value = Rc::new(value?);
        self.slots.insert(
            slot,
            Slot::Obj {
                name: name.clone(),
                value: Some(value.clone()),
            },
        );
        if &*name == "CDimensionLinear" || &*name == "CText" {
            self.annot_watermark = Some(self.next_slot);
        }
        Ok((Some(slot), Some(name), Some(value)))
    }

    /// `_translate_ref`: a FILE store-map index → the walker's numbering,
    /// through the burn bands; `None` inside a band.
    fn translate_ref(&self, slot: i64) -> Option<i64> {
        let mut offset = 0;
        for &(start, width) in &self.burns {
            if slot < start {
                break;
            }
            if slot < start + width {
                return None;
            }
            offset += width;
        }
        Some(slot - offset)
    }

    fn backref(&mut self, mut slot: i64, r: &Cursor) -> R<Read> {
        if self.burns.first().is_some_and(|b| slot >= b.0) {
            match self.translate_ref(slot) {
                // A phantom (burned) connection-point index.
                None => return Ok((Some(slot), Some(Rc::from("reserved")), None)),
                Some(walker) => slot = walker,
            }
        }
        match self.slots.get(&slot) {
            None if slot < self.walk_base => {
                // An opaque reference into the unwalked pre-model region.
                Ok((Some(slot), Some(Rc::from("premodel")), None))
            }
            None => parse_err(format!("back-ref to unwalked slot {slot} {}", r.ctx())),
            Some(Slot::Class { .. }) => {
                parse_err(format!("back-ref to class slot {slot} {}", r.ctx()))
            }
            Some(Slot::Obj { name, value }) => Ok((Some(slot), Some(name.clone()), value.clone())),
        }
    }

    fn is_class(&self, slot: i64) -> bool {
        matches!(self.slots.get(&slot), Some(Slot::Class { .. }))
    }
}

/// `_plausible_list_tag`: can the u16 at `at` start an object read?
fn plausible_list_tag(ar: &Archive, data: &[u8], at: usize) -> bool {
    let Ok(t) = u16_at(data, at) else {
        return false;
    };
    if matches!(t, 0x0000 | 0x7FFF | 0xFFFF) {
        return true;
    }
    if t & 0x8000 != 0 {
        return ar.is_class(i64::from(t & 0x7FFF));
    }
    i64::from(t) < ar.next_slot
}

/// `_retry_count_after_v20_filler`: SketchUp 2020 writes an empty string
/// record and zero padding ahead of some counts. Finds the real count
/// after it (at most `limit`), moving the cursor past it; `None` when
/// this is not that layout. `count_pos` is where the count was read.
pub fn retry_count_after_v20_filler(
    r: &mut Cursor,
    count_pos: usize,
    limit: u32,
    ar: Option<&Archive>,
) -> Option<u32> {
    let data = r.data;
    let window_end = (count_pos + 12).min(data.len().saturating_sub(3));
    let marker_at = (count_pos..window_end).find(|&i| &data[i..i + 3] == STR_MARKER)?;
    if data[marker_at + 3] != 0 {
        // A non-empty string: real data.
        return None;
    }
    // A count divisible by 256 has a zero low byte the padding swallows:
    // try the first non-zero byte and up to three positions before it.
    let mut at = marker_at + 4;
    while at < data.len() && data[at] == 0 {
        at += 1;
    }
    for back in 0..4 {
        let Some(at2) = at.checked_sub(back) else {
            continue;
        };
        if at2 < marker_at + 4 || at2 + 4 > data.len() {
            continue;
        }
        let count = u32_at(data, at2).ok()?;
        if !(0 < count && count <= limit) {
            continue;
        }
        if ar.is_some_and(|ar| !plausible_list_tag(ar, data, at2 + 4)) {
            continue;
        }
        r.pos = at2 + 4;
        return Some(count);
    }
    None
}

/// `_is_class_ref`: is the tag at `p` a class-ref to class `slot`, in
/// either encoding?
pub fn is_class_ref(data: &[u8], p: usize, slot: i64) -> bool {
    if slot < 0x7FFF {
        return u16_at(data, p).is_ok_and(|t| i64::from(t) == (0x8000 | slot));
    }
    u16_at(data, p).is_ok_and(|t| t == 0x7FFF)
        && u32_at(data, p + 2).is_ok_and(|b| i64::from(b) == (0x8000_0000 | slot))
}

// ── record readers ──────────────────────────────────────────────────────

/// `_READERS` (with the Python code's `CDimensionLinear` fix).
fn is_known_class(name: &str) -> bool {
    matches!(
        name,
        "CVertex"
            | "CEdge"
            | "CCurve"
            | "CArcCurve"
            | "CEdgeUse"
            | "CLoop"
            | "CFace"
            | "CLayer"
            | "CMaterial"
            | "CDib"
            | "CAttributeContainer"
            | "CAttributeNamed"
            | "CCamera"
            | "CThumbnail"
            | "CRelationship"
            | "CComponentDefinition"
            | "CImage"
            | "CComponentInstance"
            | "CGroup"
            | "CFaceTextureCoords"
            | "CConstructionLine"
            | "CConstructionPoint"
            | "CSectionPlane"
            | "CSkFont"
            | "CDimensionLinear"
            | "CText"
    )
}

fn read_class<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>, name: &str) -> R<Obj> {
    match name {
        "CVertex" => read_vertex(ar, r),
        "CEdge" => read_edge(ar, r),
        "CCurve" => read_curve(ar, r),
        "CArcCurve" => read_arccurve(ar, r),
        "CEdgeUse" => read_edgeuse(ar, r),
        "CLoop" => read_loop(ar, r),
        "CFace" => read_face(ar, r),
        "CLayer" => read_layer(ar, r),
        "CMaterial" => read_material(ar, r),
        "CDib" => read_dib(r),
        "CAttributeContainer" => read_attr_container(ar, r),
        "CAttributeNamed" => read_attr_named(ar, r),
        "CCamera" => read_camera(r),
        "CThumbnail" => read_thumbnail(ar, r),
        "CRelationship" => read_relationship(ar, r),
        "CComponentDefinition" => read_definition(ar, r),
        "CImage" => read_image(ar, r),
        "CComponentInstance" | "CGroup" => read_instance(ar, r),
        "CFaceTextureCoords" => read_ftc(ar, r),
        "CConstructionLine" => read_constructionline(ar, r),
        "CConstructionPoint" => read_constructionpoint(ar, r),
        "CSectionPlane" => read_sectionplane(ar, r),
        "CSkFont" => read_skfont(ar, r),
        "CDimensionLinear" => read_dimlinear(ar, r),
        "CText" => read_text(ar, r),
        _ => parse_err(format!("no reader for class {name} {}", r.ctx())),
    }
}

/// `_preamble`: the attribute pointer, plus the persistent-id mask and
/// bytes on 2017+. Returns the attribute container.
fn preamble<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Option<Rc<Obj>>> {
    let (_, _, attrs) = ar.read_object(r, Some("CAttributeContainer"))?;
    if ar.has_pid {
        let mask = r.u8()?;
        for bit in 0..8 {
            if mask & (1 << bit) != 0 {
                r.u8()?;
            }
        }
    }
    Ok(attrs)
}

/// `_drawbase`. The layer is normally a u16 id, but an entity can carry
/// it BY OBJECT (an inline `CLayer` on first use, an escaped back-ref
/// later); the id is then the default 0.
fn drawbase<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<DrawBase> {
    let b = r.raw(8);
    let lay_cls = ar.class_slot.get("CLayer").copied();
    let tag = r.peek_u16()?;
    let layer = if lay_cls.is_some_and(|c| i64::from(tag) == (0x8000 | c)) {
        ar.read_object(r, Some("CLayer"))?;
        0
    } else if tag == 0x7FFF {
        r.u16()?;
        let big = r.u32()?;
        if big & 0x8000_0000 != 0 {
            return parse_err(format!("drawbase layer: unexpected class {}", r.ctx()));
        }
        0
    } else {
        r.u16()?
    };
    let mat = u16_at(b, 0)?;
    let byte = |i: usize| {
        b.get(i)
            .copied()
            .ok_or_else(|| LegacyError::Struct("index out of range".into()))
    };
    Ok(DrawBase {
        mat,
        hidden: byte(2)?,
        soft: byte(5)?,
        smooth: byte(6)?,
        layer,
    })
}

fn read_vertex<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    Ok(Obj::Vertex { xyz: r.f64s()? })
}

fn read_edge<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let (s1, _, _) = ar.read_object(r, Some("CVertex"))?;
    let (s2, _, _) = ar.read_object(r, Some("CVertex"))?;
    let (cs, cn, cv) = ar.read_object(r, None)?;
    if !matches!(cn.as_deref(), None | Some("CCurve" | "CArcCurve")) {
        return parse_err(format!(
            "edge curve pointer resolved to {} {}",
            cn.as_deref().unwrap_or("None"),
            r.ctx()
        ));
    }
    // The curve's VALUE too: the first edge of an arc carries the only
    // inline copy of its CArcCurve.
    Ok(Obj::Edge {
        db,
        curve: cs,
        curve_value: cv,
        v1: s1,
        v2: s2,
    })
}

fn read_curve<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    r.u8()?;
    Ok(Obj::Curve { n: r.u32()? })
}

/// `_read_arccurve` (schema 3): `center[3], normal[3], x_axis[3],
/// start_angle, end_angle, y_axis[3]`.
fn read_arccurve<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    r.raw(5);
    let v: [f64; 14] = r.f64s()?;
    Ok(Obj::ArcCurve(ArcParams {
        center: [v[0], v[1], v[2]],
        normal: [v[3], v[4], v[5]],
        x_axis: [v[6], v[7], v[8]],
        start_angle: v[9],
        end_angle: v[10],
        y_axis: [v[11], v[12], v[13]],
    }))
}

/// `_register_burn`: the writer burned `delta` store-map indices for an
/// annotation's connection points without writing any bytes.
fn register_burn(ar: &mut Archive, delta: i64) {
    let watermark = ar.annot_watermark.unwrap_or(0);
    ar.burns.push((watermark + ar.cum_delta, delta));
    ar.cum_delta += delta;
    ar.annot_watermark = None;
    // Each burn is one phantom entity the list's declared count includes.
    if let Some(credit) = ar.burn_stack.last_mut() {
        *credit += 1;
    }
}

fn read_edgeuse<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let (es, _, _) = ar.read_object(r, Some("CEdge"))?;
    let sense = r.u8()?;
    // The parent-loop back-ref, read as a RAW file index: the alignment
    // oracle (it runs ahead of the walker after burned indices).
    let p0 = r.pos;
    let tag = r.u16()?;
    let ps = if tag == 0x7FFF {
        let ps = r.u32()?;
        if ps & 0x8000_0000 != 0 {
            return parse_err(format!("edge-use parent is a new object {}", r.ctx()));
        }
        Some(i64::from(ps))
    } else if tag == 0xFFFF || tag & 0x8000 != 0 {
        return parse_err(format!("edge-use parent is a new object {}", r.ctx()));
    } else {
        (tag != 0).then_some(i64::from(tag))
    };
    let expected = ar.current_loop.map(|l| l + ar.cum_delta);
    if ps != expected {
        let delta = match (ps, expected) {
            (Some(ps), Some(e)) => ps - e,
            _ => 0,
        };
        if 0 < delta && delta <= 4096 && ar.annot_watermark.is_some() {
            register_burn(ar, delta);
        } else {
            r.pos = p0;
            let show = |v: Option<i64>| v.map_or("None".to_string(), |v| v.to_string());
            return parse_err(format!(
                "edge-use parent slot {} != current loop {} {}",
                show(ps),
                show(expected),
                r.ctx()
            ));
        }
    }
    Ok(Obj::EdgeUse { edge: es, sense })
}

fn read_loop<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    let my_slot = ar.next_slot - 1;
    let prev = ar.current_loop;
    ar.current_loop = Some(my_slot);
    preamble(ar, r)?;
    r.raw(2);
    let mut uses = Vec::new();
    loop {
        if r.peek_u16()? == 0 {
            r.pos += 2;
            break;
        }
        let (_, _, v) = ar.read_object(r, Some("CEdgeUse"))?;
        uses.push(v);
    }
    // Restored on success only, as in Python.
    ar.current_loop = prev;
    Ok(Obj::Loop { uses })
}

fn read_face<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    let attrs = preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let plane = r.f64s()?;
    let nloops = r.u32()?;
    if nloops > 10000 {
        return parse_err(format!("implausible loop count {nloops} {}", r.ctx()));
    }
    let mut loops = Vec::new();
    for _ in 0..nloops {
        let (_, _, v) = ar.read_object(r, Some("CLoop"))?;
        loops.push(v);
    }
    // Edges first inlined inside this face's loops follow as redundant
    // back-ref list items; the entity list consumes them.
    let back_mat = r.u16()?;
    Ok(Obj::Face {
        db,
        plane,
        loops,
        back_mat,
        attrs,
    })
}

fn read_attr_container<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let mut children = Vec::new();
    loop {
        if r.peek_u16()? == 0 {
            r.pos += 2;
            break;
        }
        let (_, n, v) = ar.read_object(r, Some("CAttributeNamed"))?;
        children.push((n, v));
    }
    Ok(Obj::Attrs { children })
}

fn read_typed<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>, t: u8) -> R<AttrValue> {
    Ok(match t {
        0x00 => AttrValue::Null,
        0x04 => {
            let b = r.raw(4);
            if b.len() < 4 {
                return Err(LegacyError::Struct(
                    "unpack requires a buffer of 4 bytes".into(),
                ));
            }
            AttrValue::Int(i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])))
        }
        0x06 | 0x0C => AttrValue::Float(r.f64()?),
        0x07 => AttrValue::Int(i64::from(r.u8()?)),
        // time_t
        0x09 => AttrValue::Int(i64::from(r.u32()?)),
        0x0A => AttrValue::Str(r.utf16()?),
        0x0B => {
            let n = r.u32()?;
            if n > 100_000 {
                return parse_err(format!("implausible attr array count {}", r.ctx()));
            }
            if ar.depth >= MAX_DEPTH {
                return Err(LegacyError::Fatal(
                    "maximum recursion depth exceeded".into(),
                ));
            }
            ar.depth += 1;
            let mut items = Vec::new();
            let mut result = Ok(());
            for _ in 0..n {
                match r.u8().and_then(|t| read_typed(ar, r, t)) {
                    Ok(v) => items.push(v),
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            ar.depth -= 1;
            result?;
            AttrValue::List(items)
        }
        // Point3d, Vector3d
        0x11 | 0x12 => AttrValue::Triple(r.f64s()?),
        _ => {
            return parse_err(format!("unknown attribute value type {t:#x} {}", r.ctx()));
        }
    })
}

fn read_attr_named<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    r.raw(4);
    let name = r.utf16()?;
    let mut entries = IndexMap::new();
    loop {
        let key = r.utf16()?;
        if key.is_empty() {
            break;
        }
        let t = r.u8()?;
        let v = read_typed(ar, r, t)?;
        entries.insert(key, v);
    }
    r.u32()?;
    Ok(Obj::Dict { name, entries })
}

fn rgba4(b: &[u8]) -> [u8; 4] {
    std::array::from_fn(|i| b.get(i).copied().unwrap_or(0))
}

fn read_layer<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let name = r.utf16()?;
    // Flags: 3 bytes on v16, 4 on v17+. (At the end of the data Python
    // spins here forever; this stops and fails on the string below.)
    let mut mid = Vec::new();
    while mid.len() < 8 && r.peek(3) != STR_MARKER && r.pos < r.data.len() {
        mid.extend_from_slice(r.raw(1));
    }
    r.utf16()?; // internal name ("Layer_<name>")
    let flags = r.u16()?;
    let hidden = mid.first().copied().unwrap_or(0);
    if flags & 0x00FF != 0 {
        // Colour by layer with a TEXTURED material: the texture block a
        // CMaterial carries.
        let tex = texture_block(ar, r)?;
        r.raw(4);
        return Ok(Obj::Layer {
            name,
            hidden,
            rgba: tex.rgba,
        });
    }
    let rgba = rgba4(r.raw(4));
    r.utf16()?;
    r.raw(21);
    Ok(Obj::Layer { name, hidden, rgba })
}

/// `_texture_block`: an embedded `CDib` plus the applied size, source file
/// name, average colour and opacity.
fn texture_block<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<TextureBlock> {
    r.raw(if ar.ver >= 17 { 2 } else { 1 });
    let (s, _, dib) = ar.read_object(r, Some("CDib"))?;
    if !matches!(dib.as_deref(), Some(Obj::Dib { .. })) {
        return parse_err(format!("texture object is not a dib {}", r.ctx()));
    }
    // An optional u32 between the dib and the 2 x f64 applied size.
    let marker = find(r.data, STR_MARKER, r.pos, r.pos + 28);
    match marker.map(|m| m as i64 - r.pos as i64) {
        Some(20) => {
            r.u32()?;
        }
        Some(16) => {}
        _ => return parse_err(format!("texture size block misaligned {}", r.ctx())),
    }
    let w = r.f64()?;
    let h = r.f64()?;
    let fname = r.utf16()?;
    let avg = r.raw(9); // RGBA + 00 + RGBA (colour stored twice)
    r.utf16()?;
    let blob = r.raw(8); // u32 + u32 colorized flag
    let opacity = r.f64()?;
    let use_opacity = r.u8()?;
    let blob4 = blob
        .get(4)
        .copied()
        .ok_or_else(|| LegacyError::Struct("index out of range".into()))?;
    let avg3 = avg
        .get(3)
        .copied()
        .ok_or_else(|| LegacyError::Struct("index out of range".into()))?;
    Ok(TextureBlock {
        rgba: rgba4(avg),
        opacity,
        use_opacity,
        tex_dib: s,
        tex_w: w,
        tex_h: h,
        tex_file: fname,
        // A colourized texture: the second blob u32, or alpha 0xFF on the
        // stored colour.
        colorized: blob4 != 0 || avg3 == 0xFF,
    })
}

fn read_material<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let name = r.utf16()?;
    let texflag = r.u16()?;
    if texflag == 0 {
        let rgba = rgba4(r.raw(4));
        r.utf16()?; // texture path (empty)
        r.raw(8);
        let opacity = r.f64()?;
        let use_opacity = r.u8()?;
        return Ok(Obj::Material {
            name,
            rgba,
            opacity,
            use_opacity,
            texture: None,
        });
    }
    let tex = texture_block(ar, r)?;
    Ok(Obj::Material {
        name,
        rgba: tex.rgba,
        opacity: tex.opacity,
        use_opacity: tex.use_opacity,
        texture: Some(tex),
    })
}

fn read_dib(r: &mut Cursor) -> R<Obj> {
    let subtype = r.u32()?;
    let length = r.u32()? as usize;
    if length > r.data.len() {
        return parse_err(format!("implausible dib length {length} {}", r.ctx()));
    }
    Ok(Obj::Dib {
        subtype,
        data: r.raw(length).to_vec(),
    })
}

/// `_read_ftc` (`CFaceTextureCoords`): the mapping matrices and pins; bit
/// 1 of each side's flags marks a PROJECTED texture.
fn read_ftc<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    r.u32()?;
    let ks: [f64; 24] = r.f64s()?;
    for _ in 0..2 {
        let n = r.u32()?;
        for _ in 0..n {
            let _pin: [f64; 4] = r.f64s()?;
        }
    }
    let fflags = r.u32()?;
    let bflags = r.u32()?;
    Ok(Obj::Ftc {
        front: std::array::from_fn(|i| ks[i]),
        back: std::array::from_fn(|i| ks[12 + i]),
        front_projected: fflags & 2 != 0,
        back_projected: bflags & 2 != 0,
    })
}

fn read_camera(r: &mut Cursor) -> R<Obj> {
    r.raw(137);
    r.u16()?;
    r.utf16()?;
    r.raw(33);
    Ok(Obj::Camera)
}

fn read_thumbnail<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    ar.read_object(r, Some("CCamera"))?;
    ar.read_object(r, Some("CDib"))?;
    Ok(Obj::Thumbnail)
}

/// `_read_image` (`CImage`): a back-ref to the image's definition, a 3×4
/// placement, a constant 1.0, the source path and a GUID.
fn read_image<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let (ds, _, _) = ar.read_object(r, None)?;
    let xform = r.f64s()?;
    r.f64()?;
    r.utf16()?;
    let guid = hex_upper(r.raw(16));
    Ok(Obj::Image {
        db,
        def: ds,
        xform,
        guid,
    })
}

/// `_read_relationship`: two entity references, which may point forward.
fn read_relationship<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    entity_ref(r)?;
    entity_ref(r)?;
    Ok(Obj::Relationship)
}

/// `_strict_next_tag`: does the u16 at `at` start an object read in an
/// UNAMBIGUOUS form (null, escape, class definition, class-ref)?
fn strict_next_tag(ar: &Archive, data: &[u8], at: usize, allow_null: bool) -> bool {
    let Ok(t) = u16_at(data, at) else {
        return false;
    };
    match t {
        0x0000 => allow_null,
        0x7FFF | 0xFFFF => true,
        t if t & 0x8000 != 0 => ar.is_class(i64::from(t & 0x7FFF)),
        _ => false,
    }
}

fn read_constructionline<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let point: Point = r.f64s()?;
    let direction: Point = r.f64s()?;
    // Signed distances along `direction` where the visible segment starts
    // and ends; ±1e30 means unbounded.
    let [start_param, end_param] = r.f64s()?;
    // The trailing block varies by the writing build (7, 4 or 0 bytes):
    // calibrate on the file's first guide line, on the length that lands
    // on a legitimate next tag.
    let k = match ar.cline_tail {
        Some(k) => k,
        None => {
            let default = if ar.ver == 17 { 7 } else { 4 };
            let mut order = vec![default];
            order.extend([0, 4, 7].into_iter().filter(|&c| c != default));
            let mut k = None;
            // A zero tail full of padding can mimic a null tag: accept a
            // null-anchored candidate only when no strong form lands.
            'passes: for allow_null in [false, true] {
                for &cand in &order {
                    if strict_next_tag(ar, r.data, r.pos + cand, allow_null) {
                        k = Some(cand);
                        break 'passes;
                    }
                }
            }
            let k = k.unwrap_or(default);
            ar.cline_tail = Some(k);
            k
        }
    };
    r.raw(k);
    let huge = 1e20;
    let along = |t: f64| {
        (t.abs() < huge || t.is_nan()).then(|| std::array::from_fn(|i| point[i] + direction[i] * t))
    };
    Ok(Obj::Cline {
        db,
        point,
        direction,
        start: along(start_param),
        end: along(end_param),
    })
}

fn read_constructionpoint<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let pos = r.f64s()?;
    let _reserved: Point = r.f64s()?;
    r.u8()?;
    Ok(Obj::Cpoint { db, pos })
}

fn read_sectionplane<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    // An optional object pointer before the plane: a real plane starts
    // with a unit-normal component, a tag word does not decode as one.
    let first = f64_at(r.data, r.pos)?;
    // `not abs(first) <= 1.0001`: a NaN reads as an object pointer too.
    if first.abs() > 1.0001 || first.is_nan() {
        ar.read_object(r, None)?;
    }
    let plane: [f64; 4] = r.f64s()?;
    let (mut name, mut label) = (String::new(), String::new());
    if r.peek(3) == STR_MARKER {
        name = r.utf16()?;
        label = r.utf16()?;
    }
    Ok(Obj::SectionPlane {
        db,
        plane: plane.to_vec(),
        name,
        label,
    })
}

fn read_skfont<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    ar.read_object(r, Some("CAttributeContainer"))?;
    if ar.has_pid {
        let mask = r.u8()?;
        r.raw(mask.count_ones() as usize);
    }
    r.utf16()?;
    r.raw(15);
    Ok(Obj::Font)
}

/// `_entity_ref`: a reference to an entity, which may point FORWARD of
/// the walk; `None` for a null reference.
fn entity_ref(r: &mut Cursor) -> R<Option<i64>> {
    let tag = r.u16()?;
    if tag == 0 {
        return Ok(None);
    }
    if tag == 0x7FFF {
        let big = r.u32()?;
        if big & 0x8000_0000 != 0 {
            return parse_err(format!("entity ref is a new object {}", r.ctx()));
        }
        return Ok(Some(i64::from(big)));
    }
    if tag == 0xFFFF || tag & 0x8000 != 0 {
        return parse_err(format!("entity ref is a new object {}", r.ctx()));
    }
    Ok(Some(i64::from(tag)))
}

/// One reference of a dimension's instance path. MFC writes an object in
/// full the first time anything points at it, so a dimension serialized
/// before the group it is anchored in carries the whole group here.
fn path_ref<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Option<i64>> {
    let tag = r.peek_u16()?;
    let mut new = tag == 0xFFFF || (tag != 0x7FFF && tag & 0x8000 != 0);
    if tag == 0x7FFF {
        let big = u32_at(r.data, r.pos + 2)?;
        new = big & 0x8000_0000 != 0;
    }
    if new {
        let (slot, _, _) = ar.read_object(r, None)?;
        return Ok(slot);
    }
    entity_ref(r)
}

/// A connection's anchored-entity reference and its two path lists.
pub fn connection_paths<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<ConnectionPaths> {
    let extra = entity_ref(r)?;
    let mut lists = [Vec::new(), Vec::new()];
    for list in &mut lists {
        let count = r.u32()?;
        if count > 1000 {
            return parse_err(format!("implausible dimension path length {}", r.ctx()));
        }
        for _ in 0..count {
            list.push(path_ref(ar, r)?);
        }
    }
    let [p1, p2] = lists;
    Ok((extra, p1, p2))
}

/// `CDimensionLinear`, reading the connections' instance paths (see the
/// module docs). Connection blocks are `[.. u32 TYPE ..][u32 4][point3d]`:
/// type 1 is a free point stored inline, type 2 anchored (zeroed).
fn read_dimlinear<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let text = r.utf16()?;
    ar.read_object(r, Some("CSkFont"))?;
    let b37 = r.raw(37);
    let c1 = entity_ref(r)?;
    connection_paths(ar, r)?;
    let b32 = r.raw(32);
    let c2 = entity_ref(r)?;
    connection_paths(ar, r)?;
    let b72 = r.raw(72);
    let offset = f64_at(b72, 52)?;
    let a = if u32_at(b37, 5)? == 1 {
        Some(vec3_at(b37, 13)?)
    } else {
        None
    };
    let b = if u32_at(b32, 0)? == 1 {
        Some(vec3_at(b32, 8)?)
    } else {
        None
    };
    Ok(Obj::Dimension {
        db,
        text,
        connect: (c1, c2),
        offset,
        a,
        b,
    })
}

/// `data[start:end]` with Python's negative-start wrap.
fn py_slice(data: &[u8], start: i64, end: usize) -> &[u8] {
    let len = data.len() as i64;
    let s = if start < 0 {
        (len + start).max(0)
    } else {
        start
    } as usize;
    let e = end.min(data.len());
    data.get(s..e).unwrap_or_default()
}

fn read_text<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    ar.read_object(r, Some("CSkFont"))?;
    // A variable-length middle, delimited by an 11-byte block
    // `01 00 00 00 ?? 00 03 00 00 00 01` right before the text string.
    let mut p = r.pos;
    let idx = loop {
        let Some(idx) = find(r.data, STR_MARKER, p, r.pos + 512) else {
            return parse_err(format!("text delimiter not found {}", r.ctx()));
        };
        let blk = py_slice(r.data, idx as i64 - 11, idx);
        let arrow = u32_at(blk, 6)?;
        if blk.starts_with(b"\x01\x00\x00\x00") && arrow <= 8 {
            let b10 = *blk
                .get(10)
                .ok_or_else(|| LegacyError::Struct("index out of range".into()))?;
            if b10 == 1 {
                break idx;
            }
        }
        p = idx + 3;
    };
    // The middle carries the free-connection block dimensions use:
    // [u32 type=1][u32 4][point3d]; only type 1 stores the anchor.
    let mut point = None;
    let mut label = None;
    let mid = py_slice(r.data, r.pos as i64, idx);
    if let Some(off) = find(mid, b"\x01\x00\x00\x00\x04\x00\x00\x00", 0, mid.len()) {
        if off + 32 <= mid.len() {
            point = Some(vec3_at(mid, off + 8)?);
            // [12B zeros][point3d LABEL]…; an all-zero label = screen text.
            let lo = off + 32 + 12;
            if lo + 24 <= mid.len() {
                let lp = vec3_at(mid, lo)?;
                if lp.iter().any(|c| c.abs() > 1e-12) {
                    label = Some(lp);
                }
            }
        }
    }
    r.raw(idx.saturating_sub(r.pos));
    let text = r.utf16()?;
    r.raw(5);
    // Optional leader-attachment refs, escaped 6-byte form only.
    while r.peek(2) == b"\xff\x7f" {
        let val = u32_at(r.data, r.pos + 2)?;
        if val & 0x8000_0000 != 0 {
            break;
        }
        r.raw(6);
    }
    Ok(Obj::Text {
        db,
        text,
        p: point,
        lp: label,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Root,
    Def,
}

/// `_read_entity_list`: `count` items, with the burned-index credit of
/// this list on `burn_stack` while it reads.
fn read_entity_list<'d>(
    ar: &mut Archive<'d>,
    r: &mut Cursor<'d>,
    count: u32,
    owner: Owner,
) -> R<Vec<Ent>> {
    ar.burn_stack.push(0);
    let out = read_entity_list_inner(ar, r, count, owner);
    ar.burn_stack.pop();
    out
}

fn read_entity_list_inner<'d>(
    ar: &mut Archive<'d>,
    r: &mut Cursor<'d>,
    count: u32,
    owner: Owner,
) -> R<Vec<Ent>> {
    let mut ents = Vec::new();
    let burned = |ar: &Archive| ar.burn_stack.last().is_some_and(|&c| c != 0);
    while ents.len() < count as usize {
        let p = r.pos;
        if owner == Owner::Def
            && burned(ar)
            && u32_at(r.data, p)? == 0
            && py_slice(r.data, (p + 22) as i64, p + 25) == STR_MARKER
        {
            // The definition tail signature marks the list's REAL end.
            break;
        }
        let prev_flag = ar.in_entity_list;
        ar.in_entity_list = true;
        let item = ar.read_object(r, None);
        ar.in_entity_list = prev_flag;
        match item {
            Ok(item) => ents.push(item),
            // An over-declared root count runs into the document tail; a
            // list with burned indices counted phantom items: stop at the
            // failed item.
            Err(LegacyError::Parse(_))
                if owner == Owner::Root || (owner == Owner::Def && burned(ar)) =>
            {
                r.pos = p;
                break;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(ents)
}

fn read_definition<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    preamble(ar, r)?;
    r.raw(if ar.ver >= 17 { 22 } else { 20 });
    let nlayers = r.u32()?;
    if nlayers > 10000 {
        return parse_err(format!("implausible def layer count {}", r.ctx()));
    }
    // The count is REAL layers; SketchUp 2020 interleaves null separators.
    let mut got = 0;
    while got < nlayers {
        if r.peek_u16()? == 0 {
            r.pos += 2;
            continue;
        }
        ar.read_object(r, Some("CLayer"))?;
        got += 1;
    }
    let decl = r.u16()?;
    if decl == 0x7FFF {
        r.u32()?;
    }
    // v20 can drop its filler right here.
    let mut count = None;
    if ar.ver >= 20 {
        let pos = r.pos;
        count = retry_count_after_v20_filler(r, pos, 5_000_000, Some(ar));
    }
    let mut count = match count {
        Some(c) => c,
        None => {
            r.u32()?;
            r.u32()?
        }
    };
    // A zero count is as much a symptom of the v20 filler as a huge one.
    if count > 5_000_000 || count == 0 {
        let pos = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(r, pos, 5_000_000, Some(ar)) {
            count = retry;
        }
    }
    if count > 5_000_000 {
        return parse_err(format!("implausible def entity count {}", r.ctx()));
    }
    let ents = read_entity_list(ar, r, count, Owner::Def)?;
    let mut nrel = r.u32()?;
    if nrel > 100_000 {
        let pos = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(r, pos, 100_000, Some(ar)) {
            nrel = retry;
        }
    }
    if nrel > 100_000 {
        return parse_err(format!("definition list misaligned {}", r.ctx()));
    }
    for _ in 0..nrel {
        ar.read_object(r, Some("CRelationship"))?;
    }
    r.u16()?;
    // The GUID is followed by the name string; SketchUp 2020 can put two
    // extra bytes ahead of it, so anchor on the string marker.
    if r.peek(19).get(16..19) != Some(&STR_MARKER[..]) {
        for skip in 1..5 {
            let at = r.pos + skip;
            if py_slice(r.data, (at + 16) as i64, at + 19) == STR_MARKER {
                r.pos = at;
                break;
            }
        }
    }
    let guid = hex_upper(r.raw(16));
    let name = r.utf16()?;
    r.utf16()?;
    r.utf16()?;
    r.u32()?; // timestamp
              // An undecoded block, then the CThumbnail object.
    let thumb_slot = ar.class_slot.get("CThumbnail").copied();
    let mut tpos = None;
    for off in 0..96 {
        let p = r.pos + off;
        let at = |s: usize, e: usize| py_slice(r.data, (p + s) as i64, p + e);
        if at(0, 2) == b"\xff\xff" && at(4, 6) == b"\x0a\x00" && at(6, 16) == b"CThumbnail" {
            tpos = Some(p);
            break;
        }
        if thumb_slot.is_some_and(|s| is_class_ref(r.data, p, s)) {
            tpos = Some(p);
            break;
        }
    }
    let Some(tpos) = tpos else {
        return parse_err(format!("definition tail: thumbnail not found {}", r.ctx()));
    };
    let gap = r.raw(tpos - r.pos);
    // Behaviour flags 9 bytes before the thumbnail: bit 0 faces camera,
    // bit 1 shadows face sun.
    let behavior = if gap.len() >= 9 {
        gap[gap.len() - 9]
    } else {
        0
    };
    ar.read_object(r, Some("CThumbnail"))?;
    Ok(Obj::Definition {
        name,
        guid,
        ents,
        faces_camera: behavior & 1 != 0,
        shadows_face_sun: behavior & 2 != 0,
    })
}

/// `_read_instance` (`CComponentInstance` and `CGroup`). The trailing
/// GUID exists from SketchUp 2014 (version 14) on, whatever the schema.
fn read_instance<'d>(ar: &mut Archive<'d>, r: &mut Cursor<'d>) -> R<Obj> {
    let attrs = preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let (ds, dn, _) = ar.read_object(r, Some("CComponentDefinition"))?;
    if dn.as_deref() != Some("CComponentDefinition") {
        return parse_err(format!(
            "instance definition ref is {} {}",
            dn.as_deref().unwrap_or("None"),
            r.ctx()
        ));
    }
    let xf = r.f64s()?;
    let name = r.utf16()?;
    let guid = if ar.ver >= 14 {
        hex_upper(r.raw(16))
    } else {
        String::new()
    };
    Ok(Obj::Instance {
        db,
        def: ds,
        xf,
        name,
        guid,
        attrs,
    })
}

// ── pages (scenes) ──────────────────────────────────────────────────────

/// The `CViewPage` flag sets this reader understands: nothing captured,
/// or hidden layers only (`0x20`).
pub const PAGE_ALLOWED_FLAGS: [u32; 2] = [0xc00, 0xc20];
pub const PAGE_HIDDEN_LAYERS_BIT: u32 = 0x20;

/// `_skip_typed_ref`: consume one reference to a `cls_name` object
/// without needing its slot to be known.
fn skip_typed_ref<'d>(
    ar: &mut Archive<'d>,
    r: &mut Cursor<'d>,
    cls_name: &str,
    reader: fn(&mut Cursor<'d>) -> R<Obj>,
) -> R<()> {
    let tag = r.peek_u16()?;
    if tag == 0 {
        r.u16()?;
        return Ok(());
    }
    if tag == 0xFFFF {
        r.u16()?;
        let schema = r.u16()?;
        let namelen = r.u16()?;
        if namelen > 40 {
            return parse_err("implausible class name length");
        }
        let raw = r.raw(namelen as usize);
        if !raw.is_ascii() {
            return Err(LegacyError::Unicode(
                "'ascii' codec can't decode the class name".into(),
            ));
        }
        let name = std::str::from_utf8(raw).unwrap_or_default();
        if name != cls_name {
            return parse_err(format!("expected {cls_name} decl, got {name}"));
        }
        if !ar.class_slot.contains_key(cls_name) {
            let slot = ar.alloc(Slot::Class {
                name: Rc::from(cls_name),
                schema: Some(schema),
            });
            ar.class_slot.insert(cls_name.to_string(), slot);
        }
        reader(r)?;
        return Ok(());
    }
    if tag & 0x8000 != 0 {
        let cslot = i64::from(tag & 0x7FFF);
        let known = match ar.slots.get(&cslot) {
            Some(Slot::Class { name, .. }) => &**name == cls_name,
            Some(Slot::Obj { .. }) => false,
            None => {
                // Declared somewhere this scan never visited.
                ar.slots.insert(
                    cslot,
                    Slot::Class {
                        name: Rc::from(cls_name),
                        schema: None,
                    },
                );
                true
            }
        };
        if !known {
            return parse_err(format!("unexpected {cls_name} class-ref"));
        }
        r.u16()?;
        reader(r)?;
        return Ok(());
    }
    r.u16()?; // a plain back-ref
    Ok(())
}

/// `_scan_pages`: a best-effort scan of the file tail for `CViewPage`
/// records (name and hidden-layer slots), for the flag sets in
/// [`PAGE_ALLOWED_FLAGS`]. A candidate that does not fully validate is
/// skipped.
pub fn scan_pages<'d>(data: &'d [u8], ar: &mut Archive<'d>, from: usize) -> R<Vec<RawPage>> {
    let mut pages = Vec::new();
    let n = data.len();
    let mut pos = from;
    while let Some(idx) = find(data, STR_MARKER, pos, n) {
        pos = idx + 1;
        let nlen_pos = idx + 3;
        if nlen_pos >= n {
            break;
        }
        let nlen = data[nlen_pos] as usize;
        if nlen == 0 || nlen > 40 {
            continue;
        }
        let name_start = nlen_pos + 1;
        let name_end = name_start + 2 * nlen;
        if name_end + 7 > n {
            continue;
        }
        if &data[name_end..name_end + 3] != STR_MARKER || data[name_end + 3] != 0 {
            continue; // the second name must be empty
        }
        let flags_pos = name_end + 4;
        let flags = u32_at(data, flags_pos)?;
        if !PAGE_ALLOWED_FLAGS.contains(&flags) {
            continue;
        }
        let name = py::utf16le(&data[name_start..name_end], true);
        let mut r = Cursor::new(data, flags_pos + 4);
        let hidden = (|| -> R<Vec<i64>> {
            skip_typed_ref(ar, &mut r, "CCamera", read_camera)?;
            skip_typed_ref(ar, &mut r, "CDib", read_dib)?;
            let mut hidden_ids = Vec::new();
            if flags & PAGE_HIDDEN_LAYERS_BIT != 0 {
                loop {
                    let save = r.pos;
                    let v = i64::from(r.u16()?);
                    match ar.slots.get(&v) {
                        Some(Slot::Obj { name, .. }) if &**name == "CLayer" => hidden_ids.push(v),
                        _ => {
                            r.pos = save;
                            break;
                        }
                    }
                }
                if r.u32()? != 1 {
                    return parse_err("unexpected hidden-layers marker");
                }
            }
            Ok(hidden_ids)
        })();
        match hidden {
            Ok(hidden_layer_ids) => pages.push(RawPage {
                name,
                hidden_layer_ids,
                ..Default::default()
            }),
            Err(LegacyError::Parse(_) | LegacyError::Struct(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(pages)
}

// ── the walk ────────────────────────────────────────────────────────────

/// `re.search(rb'\{(\d+)\.', data[:0x60].replace(b'\x00', b''))`.
fn version_number(data: &[u8]) -> Option<i64> {
    let head: Vec<u8> = data[..data.len().min(0x60)]
        .iter()
        .copied()
        .filter(|&b| b != 0)
        .collect();
    for (i, &b) in head.iter().enumerate() {
        if b != b'{' {
            continue;
        }
        let digits = head[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits > 0 && head.get(i + 1 + digits) == Some(&b'.') {
            let s = std::str::from_utf8(&head[i + 1..i + 1 + digits]).unwrap_or("0");
            return Some(s.parse().unwrap_or(i64::MAX));
        }
    }
    None
}

/// The first `FF FF ?? ?? <u16 len> <name>` class record of `name`.
fn find_class_record(data: &[u8], name: &str) -> Option<usize> {
    let mut tail = (name.len() as u16).to_le_bytes().to_vec();
    tail.extend_from_slice(name.as_bytes());
    data.windows(4 + tail.len())
        .position(|w| w[0] == 0xFF && w[1] == 0xFF && w[4..] == tail[..])
}

/// What `_walk_model` returns.
pub struct Walked<'d> {
    pub ar: Archive<'d>,
    /// Where the main walk stopped (the page scan starts there).
    pub end: usize,
    pub root: Vec<Ent>,
    pub layers: Vec<(Option<i64>, Rc<Obj>)>,
    pub materials: Vec<(Option<i64>, Option<Rc<Obj>>)>,
}

/// `_walk`: find the model's start, the slot base, and walk the model.
pub fn walk(data: &[u8]) -> R<Walked<'_>> {
    let Some(ver) = version_number(data) else {
        return parse_err("no version string in header");
    };
    // The anchor is the material manager (the u32 count right before the
    // first CMaterial class record); a file without materials falls back
    // to the first CLayer class record.
    let (start, mat_count) = match find_class_record(data, "CMaterial") {
        Some(start) => {
            let at = start
                .checked_sub(4)
                .ok_or_else(|| LegacyError::Struct("offset out of range".into()))?;
            let mat_count = u32_at(data, at)?;
            if mat_count > 100_000 {
                return parse_err("implausible material count");
            }
            (start, mat_count)
        }
        None => {
            let Some(lm) = find_class_record(data, "CLayer") else {
                return parse_err("no CMaterial or CLayer class record found");
            };
            let back = if ver >= 17 { 9 } else { 8 };
            let start = lm
                .checked_sub(back)
                .ok_or_else(|| LegacyError::Struct("offset out of range".into()))?;
            (start, 0)
        }
    };

    let bases = if mat_count >= 2 {
        vec![bootstrap_two_materials(data, ver, start)?]
    } else {
        // Single- and no-material files: derive the base from the
        // definition-list anchor, a back-ref to the ACTIVE layer.
        probe_layer_anchor_bases(data, ver, start, mat_count)?
    };

    let mut last = None;
    for base in bases {
        match walk_model(data, ver, start, mat_count, base) {
            Ok(w) => return Ok(w),
            Err(e) if e.is_retryable() => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| LegacyError::Parse("no viable slot base candidate".into())))
}

/// `_bootstrap_two_materials`: parse material 1 with a throwaway archive;
/// material 2's class-ref tag names CMaterial's true absolute slot.
fn bootstrap_two_materials(data: &[u8], ver: i64, mat_hdr: usize) -> R<i64> {
    let mut boot = Archive::new(data, ver);
    boot.next_slot = 1 << 20;
    boot.walk_base = 1 << 20;
    let mut r = Cursor::new(data, mat_hdr);
    boot.read_object(&mut r, Some("CMaterial"))?;
    let tag = r.peek_u16()?;
    if tag == 0xFFFF || tag & 0x8000 == 0 {
        return parse_err("cannot bootstrap the slot base");
    }
    Ok(i64::from(tag & 0x7FFF))
}

/// `_probe_layer_anchor_bases`: slot-base candidates when the
/// two-material trick is unavailable. The object after the layer list is
/// an ABSOLUTE back-ref to the active layer, which the probe allocated
/// relatively: each walked layer yields one candidate.
fn probe_layer_anchor_bases(data: &[u8], ver: i64, start: usize, mat_count: u32) -> R<Vec<i64>> {
    let mut boot = Archive::new(data, ver);
    let b0: i64 = 1 << 20;
    boot.next_slot = b0;
    boot.walk_base = b0;
    let mut r = Cursor::new(data, start);
    for _ in 0..mat_count {
        boot.read_object(&mut r, Some("CMaterial"))?;
    }
    r.u32()?;
    if ver >= 17 {
        r.u8()?;
    }
    let layer_count = r.u32()?;
    if !(1..=100_000).contains(&layer_count) {
        return parse_err("implausible layer count in base probe");
    }
    let mut layer_slots = Vec::new();
    for _ in 0..layer_count {
        let (s, _, _) = boot.read_object(&mut r, Some("CLayer"))?;
        layer_slots.push(s);
    }
    let (s, n, _) = boot.read_object(&mut r, None)?;
    if n.as_deref() != Some("premodel") {
        // Under the throwaway base every absolute back-ref is premodel.
        return parse_err(format!(
            "base probe: anchor resolved to {}",
            n.as_deref().unwrap_or("None")
        ));
    }
    let s = s.unwrap_or(0);
    let mut bases = Vec::new();
    for rel in layer_slots {
        // Python subtracts None here and raises TypeError.
        let rel = rel.ok_or_else(|| LegacyError::Fatal("a null layer in the base probe".into()))?;
        let base = s - (rel - b0);
        if 0 < base && base < b0 {
            bases.push(base);
        }
    }
    Ok(bases)
}

/// `_walk_model`: the material manager, the layer list, the definition
/// list and the root entity list.
fn walk_model(data: &[u8], ver: i64, start: usize, mat_count: u32, base: i64) -> R<Walked<'_>> {
    let mut ar = Archive::new(data, ver);
    ar.next_slot = base;
    ar.walk_base = base;
    let mut r = Cursor::new(data, start);

    let mut materials = Vec::new();
    for _ in 0..mat_count {
        let (s, _, v) = ar.read_object(&mut r, Some("CMaterial"))?;
        materials.push((s, v));
    }

    // Layer list marker: v16 <u32 X><u32 count>, v17+ <u32 X><u8 0><u32 count>.
    r.u32()?;
    if ver >= 17 {
        r.u8()?;
    }
    let layer_count = r.u32()?;
    if layer_count > 100_000 {
        return parse_err("implausible layer count");
    }
    // The count is REAL layers; SketchUp 2020 interleaves null separators,
    // and a back-ref (the definition-list anchor) ends the list early.
    let mut layers = Vec::new();
    while layers.len() < layer_count as usize {
        let tag = r.peek_u16()?;
        if tag == 0 {
            r.pos += 2;
            continue;
        }
        if tag != 0xFFFF && tag & 0x8000 == 0 {
            break;
        }
        let (s, _, v) = ar.read_object(&mut r, Some("CLayer"))?;
        if let Some(v) = v {
            layers.push((s, v));
        }
    }
    // Trailing separators, and layer records past the declared count.
    let lay_cls = ar.class_slot.get("CLayer").copied();
    loop {
        let tag = r.peek_u16()?;
        if tag == 0 {
            r.pos += 2;
            continue;
        }
        if lay_cls.is_some_and(|c| i64::from(tag) == (0x8000 | c)) {
            let (s, _, v) = ar.read_object(&mut r, Some("CLayer"))?;
            if let Some(v) = v {
                layers.push((s, v));
            }
            continue;
        }
        break;
    }

    // The definition list: a pointer to the ACTIVE layer, then the count.
    let (_, dn, _) = ar.read_object(&mut r, None)?;
    if dn.as_deref() != Some("CLayer") {
        return parse_err(format!(
            "definition-list anchor is {}, not a layer",
            dn.as_deref().unwrap_or("None")
        ));
    }
    let mut def_count = r.u32()?;
    if def_count > 1_000_000 {
        let pos = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(&mut r, pos, 1_000_000, Some(&ar)) {
            def_count = retry;
        }
    }
    if def_count > 1_000_000 {
        return parse_err("implausible definition count");
    }
    for _ in 0..def_count {
        ar.read_object(&mut r, Some("CComponentDefinition"))?;
    }
    // Trailing definitions, back to back.
    let def_cls = ar.class_slot.get("CComponentDefinition").copied();
    loop {
        let tag = r.peek_u16()?;
        let mut is_def = def_cls.is_some_and(|c| i64::from(tag) == (0x8000 | c));
        if !is_def && tag == 0xFFFF && r.peek(26).get(6..26) == Some(b"CComponentDefinition") {
            is_def = true;
        }
        if !is_def {
            break;
        }
        ar.read_object(&mut r, None)?;
    }

    let mut root_count = r.u32()?;
    if root_count > 5_000_000 {
        let pos = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(&mut r, pos, 5_000_000, Some(&ar)) {
            root_count = retry;
        }
    }
    if root_count > 5_000_000 {
        return parse_err("implausible root entity count");
    }
    let root = read_entity_list(&mut ar, &mut r, root_count, Owner::Root)?;
    Ok(Walked {
        ar,
        end: r.pos,
        root,
        layers,
        materials,
    })
}

// ── adapting to the full_parse shape ────────────────────────────────────

/// `_stringify_attr_value`.
pub fn stringify_attr_value(v: &AttrValue) -> String {
    v.stringify()
}

/// `_extract_legacy_attribute_dictionaries`: every attribute dictionary of
/// an entity's `CAttributeContainer`, by its declared name.
pub fn extract_legacy_attribute_dictionaries(attrs: Option<&Obj>) -> AttrDicts {
    let mut out = IndexMap::new();
    let Some(Obj::Attrs { children }) = attrs else {
        return out;
    };
    for (_, value) in children {
        let Some(value) = value else {
            continue;
        };
        let Some(name) = value.name().filter(|n| !n.is_empty()) else {
            continue;
        };
        let entries = match &**value {
            Obj::Dict { entries, .. } => entries.clone(),
            _ => IndexMap::new(),
        };
        out.insert(name.to_string(), entries);
    }
    out
}

/// A Python `KeyError`/`TypeError` while building the definitions.
fn bad(what: &str) -> LegacyError {
    LegacyError::Fatal(format!("unexpected record shape: {what}"))
}

/// `_add_edge`: an edge and its two vertices into `builder`, once.
fn add_edge(
    builder: &mut GeometryBuilder,
    slot: i64,
    e: &Obj,
    slots: &IndexMap<i64, Slot>,
) -> R<()> {
    if builder.edges.contains_key(&slot) {
        return Ok(());
    }
    let Obj::Edge {
        db,
        curve,
        curve_value,
        v1,
        v2,
    } = e
    else {
        return Err(bad("an edge reference that is not an edge"));
    };
    for vs in [v1, v2] {
        let Some(vs) = *vs else {
            continue;
        };
        if builder.vertices.contains_key(&vs) {
            continue;
        }
        match slots.get(&vs) {
            Some(Slot::Obj { value: Some(v), .. }) => match &**v {
                Obj::Vertex { xyz } => {
                    builder.vertices.insert(vs, *xyz);
                }
                _ => return Err(bad("a vertex reference that is not a vertex")),
            },
            Some(Slot::Class {
                schema: Some(_), ..
            }) => return Err(bad("a vertex reference to a class")),
            _ => {}
        }
    }
    builder.edges.insert(slot, (*v1, *v2));
    let flags = (if db.soft != 0 { 0x08 } else { 0 })
        | (if db.smooth != 0 { 0x10 } else { 0 })
        | (if db.hidden != 0 { 0x01 } else { 0 });
    if flags != 0 {
        builder.edge_flags.insert(slot, flags);
    }
    if db.layer != 0 {
        builder.edge_layers.insert(slot, i64::from(db.layer));
    }
    if let Some(curve) = *curve {
        builder.edge_curves.insert(slot, curve);
        if let Some(Obj::ArcCurve(params)) = curve_value.as_deref() {
            builder.arc_curves.insert(curve, params.clone());
        }
    }
    Ok(())
}

/// `_fill_builder`: one entity list into a geometry builder.
fn fill_builder(builder: &mut GeometryBuilder, ents: &[Ent], slots: &IndexMap<i64, Slot>) -> R<()> {
    for (s, _, v) in ents {
        let Some(v) = v else {
            continue;
        };
        // Every dict-valued item came with its slot.
        let s = s.unwrap_or_default();
        match &**v {
            Obj::Edge { .. } => add_edge(builder, s, v, slots)?,
            Obj::ArcCurve(params) => {
                builder.arc_curves.insert(s, params.clone());
            }
            Obj::Face {
                db,
                plane,
                loops: face_loops,
                back_mat,
                attrs,
            } => {
                let mut loops = Vec::new();
                for lp in face_loops {
                    let Some(Obj::Loop { uses }) = lp.as_deref() else {
                        return Err(bad("a face loop that is not a loop"));
                    };
                    let mut lp_out = Vec::new();
                    for u in uses {
                        let Some(Obj::EdgeUse { edge, sense }) = u.as_deref() else {
                            return Err(bad("a loop item that is not an edge use"));
                        };
                        let Some(es) = *edge else {
                            continue;
                        };
                        match slots.get(&es) {
                            None
                            | Some(Slot::Obj { value: None, .. })
                            | Some(Slot::Class { schema: None, .. }) => continue,
                            Some(Slot::Obj { value: Some(e), .. }) => {
                                add_edge(builder, es, e, slots)?
                            }
                            Some(Slot::Class { .. }) => {
                                if !builder.edges.contains_key(&es) {
                                    return Err(bad("an edge reference to a class"));
                                }
                            }
                        }
                        // The raw bit is 0 forward / 1 reversed.
                        lp_out.push((es, if *sense != 0 { -1 } else { 1 }));
                    }
                    loops.push(lp_out);
                }
                let mut face = RawFace {
                    loops,
                    normal: Some([plane[0], plane[1], plane[2]]),
                    material_id: (db.mat != 0).then_some(i64::from(db.mat)),
                    back_material_id: (*back_mat != 0).then_some(i64::from(*back_mat)),
                    hidden: db.hidden != 0,
                    layer: Some(i64::from(db.layer)),
                    ..Default::default()
                };
                if let Some(Obj::Attrs { children }) = attrs.as_deref() {
                    for (_, cv) in children {
                        if let Some(Obj::Ftc {
                            front,
                            back,
                            front_projected,
                            back_projected,
                        }) = cv.as_deref()
                        {
                            face.uv_transform = Some(*front);
                            face.uv_transform_back = Some(*back);
                            face.uv_projected = *front_projected;
                            face.uv_projected_back = *back_projected;
                        }
                    }
                }
                builder.faces.insert(s, face);
            }
            Obj::Instance {
                db,
                def,
                xf,
                name,
                attrs,
                ..
            } => {
                let attr_dicts = extract_legacy_attribute_dictionaries(attrs.as_deref());
                let properties = attr_dicts
                    .get("dynamic_attributes")
                    .map(|d| d.iter().map(|(k, v)| (k.clone(), v.stringify())).collect())
                    .unwrap_or_default();
                builder.instances.push(RawInstance {
                    name: Some(name.clone()),
                    ref_idx: *def,
                    ref_guid: Some(String::new()),
                    matrix: xf.to_vec(),
                    material_id: (db.mat != 0).then_some(i64::from(db.mat)),
                    layer_id: (db.layer != 0).then_some(i64::from(db.layer)),
                    hidden: db.hidden != 0,
                    attribute_dictionaries: attr_dicts,
                    properties,
                });
            }
            // An Image entity is placed like an ordinary instance; its
            // definition is flagged `is_image`.
            Obj::Image {
                db,
                def,
                xform,
                guid,
            } => builder.instances.push(RawInstance {
                name: Some(String::new()),
                ref_idx: *def,
                ref_guid: Some(guid.clone()),
                matrix: xform.to_vec(),
                material_id: (db.mat != 0).then_some(i64::from(db.mat)),
                layer_id: (db.layer != 0).then_some(i64::from(db.layer)),
                hidden: db.hidden != 0,
                ..Default::default()
            }),
            Obj::SectionPlane {
                db,
                plane,
                name,
                label,
            } => builder.section_planes.push(RawSectionPlane {
                plane: plane.clone(),
                name: name.clone(),
                label: label.clone(),
                hidden: db.hidden != 0,
            }),
            Obj::Text { db, text, p, lp } => builder.texts.push(RawText {
                text: text.clone(),
                hidden: db.hidden != 0,
                p: *p,
                lp: *lp,
            }),
            Obj::Dimension {
                db,
                text,
                connect,
                offset,
                a,
                b,
            } => {
                // FREE connections carry their points inline; anchored
                // ones resolve to their vertices.
                let mut pts = [*a, *b];
                for (i, cs) in [connect.0, connect.1].into_iter().enumerate() {
                    if pts[i].is_some() {
                        continue;
                    }
                    if let Some(Slot::Obj {
                        value: Some(val), ..
                    }) = cs.and_then(|c| slots.get(&c))
                    {
                        if let Obj::Vertex { xyz } = &**val {
                            pts[i] = Some(*xyz);
                        }
                    }
                }
                let mut dim = RawDimension {
                    text: text.clone(),
                    hidden: db.hidden != 0,
                    offset: *offset,
                    ..Default::default()
                };
                if let [Some(a), Some(b)] = pts {
                    dim.a = Some(a);
                    dim.b = Some(b);
                }
                builder.dimensions.push(dim);
            }
            Obj::Cline {
                point,
                direction,
                start,
                end,
                ..
            } => builder.construction_lines.push(RawConstructionLine {
                point: *point,
                direction: *direction,
                start: *start,
                end: *end,
            }),
            Obj::Cpoint { pos, .. } => builder.construction_points.push(*pos),
            _ => {}
        }
    }
    Ok(())
}

/// `full_parse_legacy`: parse a classic `.skp` into the `full_parse`
/// shape.
pub fn full_parse_legacy(data: &[u8]) -> Result<Parsed, SkpParseError> {
    let mut version = "unknown".to_string();
    if let Some(second) = find(data, STR_MARKER, 4, data.len()).filter(|&s| s > 0) {
        let lo = (second + 4).min(data.len());
        let hi = (second + 100).min(data.len());
        let text = py::utf16le(&data[lo..hi], false);
        if let (Some(open), Some(close)) = (text.find('{'), text.find('}')) {
            version = if close >= open {
                text[open..=close].to_string()
            } else {
                String::new()
            };
        }
    }

    let walked = walk(data).map_err(|e| {
        SkpParseError::new(format!("legacy .skp parse failed: {e}"), Stage::LegacyWalk).caused_by(e)
    })?;
    let Walked {
        mut ar,
        end,
        root,
        layers,
        materials,
    } = walked;

    // Definitions an Image entity places: a classic file says "this is an
    // Image" only through the kind of entity that places it.
    let image_def_ids: HashSet<Option<i64>> = ar
        .slots
        .values()
        .filter_map(|s| match s {
            Slot::Obj {
                name,
                value: Some(v),
            } if &**name == "CImage" => match &**v {
                Obj::Image { def, .. } => Some(*def),
                _ => None,
            },
            _ => None,
        })
        .collect();

    let mut parsed = Parsed {
        version,
        ..Default::default()
    };

    // Materials, keyed by name like the VFF path.
    let fatal = |what: &str| {
        SkpParseError::new(
            format!("legacy .skp parse failed: {what}"),
            Stage::LegacyWalk,
        )
    };
    for (s, v) in &materials {
        let Some(v) = v else {
            return Err(fatal("a material record without a value"));
        };
        let rgba = v.rgba().unwrap_or([128, 128, 128, 255]);
        let (use_opacity, opacity, tex) = match &**v {
            Obj::Material {
                use_opacity,
                opacity,
                texture,
                ..
            } => (*use_opacity, *opacity, texture.as_ref()),
            _ => (0, 0.0, None),
        };
        // The stored f64 is a TRANSPARENCY, gated by the use flag.
        let transparency = if use_opacity != 0 {
            py::min(py::max(1.0 - opacity, 0.0), 1.0)
        } else {
            1.0
        };
        let colorized = tex.is_some_and(|t| t.colorized);
        let Some(name) = v.name() else {
            return Err(fatal("a material record without a name"));
        };
        let texture = tex.map(|t| {
            let data = t
                .tex_dib
                .and_then(|d| ar.slots.get(&d))
                .and_then(|slot| match slot {
                    Slot::Obj { value: Some(v), .. } => match &**v {
                        Obj::Dib { data, .. } => Some(data.clone()),
                        _ => None,
                    },
                    _ => None,
                });
            let ext = if data.as_deref().unwrap_or_default().starts_with(b"\x89PNG") {
                ".png"
            } else {
                ".jpg"
            };
            let filename = if t.tex_file.is_empty() {
                format!("{name}{ext}")
            } else {
                t.tex_file.clone()
            };
            Texture {
                filename,
                width: t.tex_w,
                height: t.tex_h,
                data,
            }
        });
        parsed.material_data.push(RawMaterial {
            name: name.to_string(),
            color: [i64::from(rgba[0]), i64::from(rgba[1]), i64::from(rgba[2])],
            alpha: Some(i64::from(rgba[3])),
            transparency,
            // The colourize type is not decoded; tint suits the grey base
            // textures seen.
            colorized,
            colorize_type: i64::from(colorized),
            texture,
        });
        let ix = parsed.material_data.len() - 1;
        parsed.materials.insert(name.to_string(), ix);
        if let Some(s) = s {
            parsed.material_id_to_name.insert(*s, name.to_string());
        }
    }

    for (s, v) in &layers {
        let rgba = v.rgba().unwrap_or([136, 136, 136, 255]);
        let hidden = match &**v {
            Obj::Layer { hidden, .. } => *hidden != 0,
            _ => false,
        };
        let Some(name) = v.name() else {
            return Err(fatal("a layer record without a name"));
        };
        parsed.layer_colors.insert(
            name.to_string(),
            (i64::from(rgba[0]), i64::from(rgba[1]), i64::from(rgba[2])),
        );
        parsed.layer_hidden.insert(name.to_string(), hidden);
        if let Some(s) = s {
            parsed.layer_id_to_name.insert(*s, name.to_string());
        }
    }
    parsed
        .layer_colors
        .entry("Layer0".into())
        .or_insert((136, 136, 136));
    parsed.layer_hidden.entry("Layer0".into()).or_insert(false);

    for (&s, slot) in &ar.slots {
        let Slot::Obj {
            name,
            value: Some(v),
        } = slot
        else {
            continue;
        };
        if &**name != "CComponentDefinition" {
            continue;
        }
        let Obj::Definition {
            name: def_name,
            guid,
            ents,
            faces_camera,
            shadows_face_sun,
        } = &**v
        else {
            continue;
        };
        let mut builder = GeometryBuilder::default();
        fill_builder(&mut builder, ents, &ar.slots).map_err(|e| {
            SkpParseError::new(
                format!("Failed while building component definitions: {e}"),
                Stage::LegacyDefs,
            )
            .in_definition(s)
            .caused_by(e)
        })?;
        parsed.defs.insert(
            Some(s),
            RawDefinition {
                guid: Some(guid.clone()),
                name: Some(def_name.clone()),
                is_image: image_def_ids.contains(&Some(s)),
                always_faces_camera: *faces_camera,
                shadows_face_sun: *shadows_face_sun,
                builder,
            },
        );
    }

    let mut root_builder = GeometryBuilder::default();
    fill_builder(&mut root_builder, &root, &ar.slots).map_err(|e| {
        SkpParseError::new(format!("legacy .skp parse failed: {e}"), Stage::LegacyWalk).caused_by(e)
    })?;

    parsed.pages = match scan_pages(data, &mut ar, end) {
        Ok(pages) => pages,
        Err(LegacyError::Parse(_) | LegacyError::Struct(_)) => Vec::new(),
        Err(e) => {
            return Err(SkpParseError::new(
                format!("legacy .skp parse failed: {e}"),
                Stage::LegacyWalk,
            )
            .caused_by(e))
        }
    };
    parsed.dimensions = root_builder
        .dimensions
        .iter()
        .filter(|d| d.a.is_some() && d.b.is_some())
        .cloned()
        .collect();
    parsed.root = Some(root_definition(root_builder));
    Ok(parsed)
}
