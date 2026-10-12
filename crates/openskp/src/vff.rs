// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The VFF (2021+) half of `openskp/_core.py`: `full_parse` from the
//! opened container on, the geometry extraction, attribute dictionaries,
//! scenes and dimensions.
//!
//! `model.dat` is walked one top-level record at a time
//! ([`crate::tlv::iter_top_level`]), so only one subtree is alive at once.
//! Where Python reads past the end of a payload (`struct.error`,
//! `IndexError`) inside that walk, the parse fails with an
//! [`SkpParseError`] at stage `tlv_walk`, as Python's `try` turns those
//! exceptions into one.

use std::collections::HashMap;
use std::fmt;

use indexmap::IndexMap;

use crate::container::VffArchive;
use crate::errors::{SkpParseError, Stage};
use crate::model::{AttrDicts, AttrValue, Point};
use crate::parsed::{
    root_definition, GeometryBuilder, Parsed, RawDefinition, RawDimension, RawFace, RawInstance,
    RawPage,
};
use crate::py;
use crate::tlv::{self, iter_top_level, parse_tlv_recursive, Tag, TagSet, TlvNode, CONTAINER_TAGS};
use crate::xml;

/// A read past the end of a payload: Python's `struct.error` or
/// `IndexError`.
#[derive(Debug, Clone)]
pub struct Fault(pub String);

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Fault {}

type R<T> = Result<T, Fault>;

const fn t(hex: u16) -> Tag {
    Tag::new((hex >> 8) as u8, hex as u8)
}

const C409: Tag = t(0xC409);
const C509: Tag = t(0xC509);
const B80B: Tag = t(0xB80B);
const B90B: Tag = t(0xB90B);
const BA0B: Tag = t(0xBA0B);
const BB0B: Tag = t(0xBB0B);
const AC0D: Tag = t(0xAC0D);
const AD0D: Tag = t(0xAD0D);
const AE0D: Tag = t(0xAE0D);
const AF0D: Tag = t(0xAF0D);
const T9411: Tag = t(0x9411);
const A00F: Tag = t(0xA00F);
const T6419: Tag = t(0x6419);
const T6519: Tag = t(0x6519);
const T6619: Tag = t(0x6619);
const T6719: Tag = t(0x6719);
const T6819: Tag = t(0x6819);
const D007: Tag = t(0xD007);
const D107: Tag = t(0xD107);
const D207: Tag = t(0xD207);
const D307: Tag = t(0xD307);
const DC05: Tag = t(0xDC05);
const DD05: Tag = t(0xDD05);
const DE05: Tag = t(0xDE05);
const B136: Tag = t(0xB136);
const B236: Tag = t(0xB236);
const T1027: Tag = t(0x1027);
const T1127: Tag = t(0x1127);
const T1227: Tag = t(0x1227);
const T1327: Tag = t(0x1327);
const T1527: Tag = t(0x1527);
const T993A: Tag = t(0x993A);
const T8C3C: Tag = t(0x8C3C);
const T8D3C: Tag = t(0x8D3C);
const T8E3C: Tag = t(0x8E3C);
const C832: Tag = t(0xC832);
const CC32: Tag = t(0xCC32);
const T7C15: Tag = t(0x7C15);
const T7D15: Tag = t(0x7D15);
const T7E15: Tag = t(0x7E15);
const T581B: Tag = t(0x581B);
const T5D1B: Tag = t(0x5D1B);
const T5E1B: Tag = t(0x5E1B);
const T8315: Tag = t(0x8315);
const F601: Tag = t(0xF601);
const T0702: Tag = t(0x0702);
const B436: Tag = t(0xB436);
const B636: Tag = t(0xB636);
const A438: Tag = t(0xA438);
const AD38: Tag = t(0xAD38);
const AF38: Tag = t(0xAF38);
const A938: Tag = t(0xA938);
const A738: Tag = t(0xA738);
const B438: Tag = t(0xB438);
const B538: Tag = t(0xB538);
const AE38: Tag = t(0xAE38);

/// `_PROP_CONTAINER_TAGS`: the containers of an attribute-dictionary blob.
static PROP_CONTAINER_TAGS: TagSet = TagSet::new(&[
    DD05,
    t(0xB536),
    B136,
    B236,
    t(0xB336),
    t(0xB036),
    A438,
    AE38,
]);

const DE05_PREFIX: [u8; 2] = [0xDE, 0x05];

// ── primitive reads ─────────────────────────────────────────────────────

/// `read_u32`, failing like `struct.unpack_from` past the end.
fn u32_at(data: &[u8], offset: usize) -> R<u32> {
    tlv::read_u32(data, offset).ok_or_else(|| {
        Fault(format!(
            "unpack_from requires a buffer of at least {} bytes for unpacking 4 bytes \
             at offset {offset} (actual buffer size is {})",
            offset + 4,
            data.len()
        ))
    })
}

/// `read_f64` where the caller has checked the length.
fn f64_at(data: &[u8], offset: usize) -> f64 {
    tlv::read_f64(data, offset).unwrap_or(0.0)
}

fn vec3_at(data: &[u8], offset: usize) -> Point {
    [
        f64_at(data, offset),
        f64_at(data, offset + 8),
        f64_at(data, offset + 16),
    ]
}

/// `parse_var_int`: a little-endian integer of `length` bytes, failing
/// like Python's `IndexError` past the end. Python integers are
/// unbounded; past 8 bytes only the low 64 bits are kept (exact for the
/// zero padding a longer field carries).
pub(crate) fn var_int(data: &[u8], offset: usize, length: usize) -> R<i64> {
    let bytes = offset
        .checked_add(length)
        .and_then(|end| data.get(offset..end))
        .ok_or_else(|| Fault("index out of range".into()))?;
    Ok(bytes
        .iter()
        .take(8)
        .enumerate()
        .fold(0u64, |v, (i, &b)| v | u64::from(b) << (8 * i)) as i64)
}

fn var_int_all(payload: &[u8]) -> i64 {
    var_int(payload, 0, payload.len()).unwrap_or(0)
}

// ── node helpers ────────────────────────────────────────────────────────

/// `find_child_tag`: the first node with `tag`, depth-first, pre-order.
pub(crate) fn find_child_tag<'n, 'a>(
    nodes: &'n [TlvNode<'a>],
    tag: Tag,
) -> Option<&'n TlvNode<'a>> {
    for n in nodes {
        if n.tag == tag {
            return Some(n);
        }
        if let Some(r) = find_child_tag(&n.children, tag) {
            return Some(r);
        }
    }
    None
}

/// The first DIRECT child with `tag` (`next(c for c in children if …)`).
fn direct<'n, 'a>(nodes: &'n [TlvNode<'a>], tag: Tag) -> Option<&'n TlvNode<'a>> {
    nodes.iter().find(|n| n.tag == tag)
}

/// `find_all_nodes_rec`.
fn find_all<'n, 'a>(nodes: &'n [TlvNode<'a>], tag: Tag, out: &mut Vec<&'n TlvNode<'a>>) {
    for n in nodes {
        if n.tag == tag {
            out.push(n);
        }
        find_all(&n.children, tag, out);
    }
}

/// The id a `DC05` payload carries: the `DE05` var-int inside it, or the
/// payload itself as a var-int.
fn dc05_id(payload: &[u8]) -> R<i64> {
    if payload.starts_with(&DE05_PREFIX) {
        let len = u32_at(payload, 2)? as usize;
        var_int(payload, 6, len)
    } else {
        var_int(payload, 0, payload.len())
    }
}

/// The persistent-id bytes of a `DC05` payload (`_scan_vertex_positions`).
fn dc05_id_bytes(payload: &[u8]) -> R<&[u8]> {
    if payload.starts_with(&DE05_PREFIX) {
        let len = u32_at(payload, 2)? as usize;
        Ok(slice(payload, 6, 6usize.saturating_add(len)))
    } else {
        Ok(payload)
    }
}

/// `data[start:end]` with Python's clamping.
fn slice(data: &[u8], start: usize, end: usize) -> &[u8] {
    let end = end.min(data.len());
    data.get(start.min(end)..end).unwrap_or_default()
}

/// `extract_entity_id`: the entity id among the node's children, or
/// deeper.
pub fn extract_entity_id(node: &TlvNode) -> R<Option<i64>> {
    for child in &node.children {
        if child.tag == DE05 {
            return Ok(Some(var_int_all(child.payload)));
        }
        if child.tag == DC05 && child.payload.starts_with(&DE05_PREFIX) {
            return dc05_id(child.payload).map(Some);
        }
    }
    for child in &node.children {
        if let Some(id) = extract_entity_id(child)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// `_tlv_flat`: a payload as a flat `(tag, body)` run.
fn tlv_flat(payload: &[u8]) -> Vec<(Tag, &[u8])> {
    tlv::flat_headers(payload, 0, payload.len())
        .into_iter()
        .map(|h| {
            (
                h.tag,
                &payload[h.offset + tlv::HEADER_LEN..h.offset + tlv::HEADER_LEN + h.size],
            )
        })
        .collect()
}

fn flat_find<'a>(seq: &[(Tag, &'a [u8])], tag: Tag) -> Option<&'a [u8]> {
    seq.iter().find(|(t, _)| *t == tag).map(|(_, b)| *b)
}

/// `_extract_uv_transforms`: a face's positioned-texture matrices (front,
/// back) from its `DC05` entity-info blob.
pub fn extract_uv_transforms(dc05_payload: &[u8]) -> (Option<[f64; 9]>, Option<[f64; 9]>) {
    let chain = || {
        let dd05 = flat_find(&tlv_flat(dc05_payload), DD05)?;
        let b136 = flat_find(&tlv_flat(dd05), B136)?;
        let b236 = flat_find(&tlv_flat(b136), B236)?;
        flat_find(&tlv_flat(b236), T1027)
    };
    let Some(t1027) = chain() else {
        return (None, None);
    };
    let sides = tlv_flat(t1027);
    let side = |tag| {
        let side = flat_find(&sides, tag)?;
        let t1327 = flat_find(&tlv_flat(side), T1327)?;
        let m = flat_find(&tlv_flat(t1327), T1527)?;
        (m.len() == 72).then(|| std::array::from_fn(|i| f64_at(m, i * 8)))
    };
    (side(T1127), side(T1227))
}

// ── geometry ────────────────────────────────────────────────────────────

/// `_extract_geometry_from_nodes`: vertices, edges, faces and instances
/// under `elements`, into `builder`.
pub fn extract_geometry_from_nodes(elements: &[TlvNode], builder: &mut GeometryBuilder) -> R<()> {
    for el in elements {
        match el.tag {
            C409 => {
                let v_id = extract_entity_id(el)?;
                let c509 = find_child_tag(&el.children, C509);
                if let (Some(v_id), Some(c509)) = (v_id, c509) {
                    if c509.payload.len() >= 24 {
                        builder.vertices.insert(v_id, vec3_at(c509.payload, 0));
                    }
                }
            }
            B80B => {
                if let Some(e_id) = extract_entity_id(el)? {
                    let end =
                        |tag| find_child_tag(&el.children, tag).map(|n| var_int_all(n.payload));
                    builder.edges.insert(e_id, (end(B90B), end(BA0B)));
                    // D007 -> D307 = display flags (0x01 hidden, 0x08|0x10
                    // soft/smooth); D207 = layer id.
                    if let Some(d007) = direct(&el.children, D007) {
                        if let Some(&flags) =
                            direct(&d007.children, D307).and_then(|n| n.payload.first())
                        {
                            builder.edge_flags.insert(e_id, flags);
                        }
                        if let Some(d207) =
                            direct(&d007.children, D207).filter(|n| !n.payload.is_empty())
                        {
                            builder.edge_layers.insert(e_id, var_int_all(d207.payload));
                        }
                    }
                    // BB0B = the parent Curve's entity id (Edge#curve).
                    if let Some(bb0b) =
                        find_child_tag(&el.children, BB0B).filter(|n| !n.payload.is_empty())
                    {
                        let curve_id = var_int_all(bb0b.payload);
                        if curve_id != 0 {
                            builder.edge_curves.insert(e_id, curve_id);
                        }
                    }
                }
            }
            AC0D => {
                if let Some(f_id) = extract_entity_id(el)? {
                    let face = extract_face(el);
                    builder.faces.insert(f_id, face);
                }
            }
            T6419 => builder.instances.push(extract_instance(el)?),
            _ => {
                if !el.children.is_empty() {
                    extract_geometry_from_nodes(&el.children, builder)?;
                }
            }
        }
    }
    Ok(())
}

/// The `AC0D` branch: normal, loops, materials, uv mapping, hidden, layer.
fn extract_face(el: &TlvNode) -> RawFace {
    let normal = find_child_tag(&el.children, AD0D)
        .filter(|n| n.payload.len() >= 24)
        .map_or([0.0, 0.0, 1.0], |n| vec3_at(n.payload, 0));

    let mut loops = Vec::new();
    if let Some(ae0d) = find_child_tag(&el.children, AE0D) {
        let mut loop_nodes = Vec::new();
        find_all(&ae0d.children, T9411, &mut loop_nodes);
        for ln in loop_nodes {
            let mut co_nodes = Vec::new();
            find_all(&ln.children, A00F, &mut co_nodes);
            let mut co_edges = Vec::new();
            for cn in co_nodes {
                let p = cn.payload;
                let mut edge_id = None;
                let mut orient = None;
                let mut sub_pos = 0usize;
                while sub_pos + 6 < p.len() {
                    let sub_tag = [p[sub_pos], p[sub_pos + 1]];
                    let sub_size = tlv::read_u32(p, sub_pos + 2).unwrap_or(0) as usize;
                    if sub_pos + 6 + sub_size <= p.len() {
                        let val = var_int(p, sub_pos + 6, sub_size).unwrap_or(0);
                        match sub_tag {
                            [0xA1, 0x0F] => edge_id = Some(val),
                            [0xA2, 0x0F] => orient = Some(val),
                            _ => {}
                        }
                    }
                    sub_pos += 6 + sub_size;
                }
                // The raw A20F bit is 0 forward / 1 reversed; the contract
                // is +1 / -1.
                if let (Some(e), Some(o)) = (edge_id, orient) {
                    co_edges.push((e, if o == 0 { 1 } else { -1 }));
                }
            }
            if !co_edges.is_empty() {
                loops.push(co_edges);
            }
        }
    }

    let mut face = RawFace {
        loops,
        normal: Some(normal),
        ..Default::default()
    };
    if let Some(d007) = direct(&el.children, D007) {
        if let Some(d107) = direct(&d007.children, D107) {
            face.material_id = Some(var_int_all(d107.payload));
        }
        if let Some(d207) = direct(&d007.children, D207).filter(|n| !n.payload.is_empty()) {
            face.layer = Some(var_int_all(d207.payload));
        }
        if let Some(dc05) = direct(&d007.children, DC05) {
            (face.uv_transform, face.uv_transform_back) = extract_uv_transforms(dc05.payload);
        }
        if let Some(&flags) = direct(&d007.children, D307).and_then(|n| n.payload.first()) {
            face.hidden = flags & 0x01 != 0;
        }
    }
    // AF0D = the BACK side's material.
    if let Some(af0d) = direct(&el.children, AF0D).filter(|n| !n.payload.is_empty()) {
        face.back_material_id = Some(var_int_all(af0d.payload));
    }
    face
}

/// The `6419` branch: one placed instance.
fn extract_instance(el: &TlvNode) -> R<RawInstance> {
    let own = std::slice::from_ref(el);
    let nodes: &[TlvNode] = if el.children.is_empty() {
        own
    } else {
        &el.children
    };
    let guid = find_child_tag(nodes, T6819)
        .filter(|n| n.payload.len() == 16)
        .map(|n| py::hex_upper(n.payload));
    let ref_idx = find_child_tag(nodes, T6719).map(|n| var_int_all(n.payload));
    let name = find_child_tag(nodes, T6519).map(|n| py::utf8_replace(n.payload));
    let matrix = find_child_tag(nodes, T6619)
        .filter(|n| n.payload.len() >= 104)
        .map(|n| (0..13).map(|i| f64_at(n.payload, i * 8)).collect())
        .unwrap_or_default();

    // Instance-level material ("paint the component"), layer override,
    // attribute dictionaries and hidden flag, under D007.
    let mut inst = RawInstance {
        name,
        ref_idx,
        ref_guid: guid,
        matrix,
        ..Default::default()
    };
    if let Some(d007) = direct(&el.children, D007) {
        if let Some(d107) = direct(&d007.children, D107) {
            inst.material_id = Some(var_int_all(d107.payload));
        }
        if let Some(d207) = direct(&d007.children, D207).filter(|n| !n.payload.is_empty()) {
            inst.layer_id = Some(var_int_all(d207.payload));
        }
        inst.attribute_dictionaries = extract_attribute_dictionaries(d007)?;
        if let Some(&flags) = direct(&d007.children, D307).and_then(|n| n.payload.first()) {
            inst.hidden = flags & 0x01 != 0;
        }
    }
    if let Some(dynamic) = inst.attribute_dictionaries.get("dynamic_attributes") {
        inst.properties = dynamic
            .iter()
            .map(|(k, v)| (k.clone(), v.stringify()))
            .collect();
    }
    Ok(inst)
}

// ── transforms ──────────────────────────────────────────────────────────

/// `transform_point`: `p` through a 13-float instance matrix.
pub fn transform_point(p: Point, m: &[f64]) -> Point {
    if m.len() < 12 {
        return p;
    }
    let [x, y, z] = p;
    [
        m[0] * x + m[1] * y + m[2] * z + m[9],
        m[3] * x + m[4] * y + m[5] * z + m[10],
        m[6] * x + m[7] * y + m[8] * z + m[11],
    ]
}

/// `multiply_matrices`: parent × child, both 13-float matrices; an empty
/// side yields the other.
pub fn multiply_matrices(parent: &[f64], child: &[f64]) -> Vec<f64> {
    if parent.is_empty() {
        return child.to_vec();
    }
    if child.is_empty() {
        return parent.to_vec();
    }
    let g = |m: &[f64], i: usize| m.get(i).copied().unwrap_or(0.0);
    let p = |i| g(parent, i);
    let c = |i| g(child, i);
    let rows = [
        [p(0), p(1), p(2), p(9)],
        [p(3), p(4), p(5), p(10)],
        [p(6), p(7), p(8), p(11)],
    ];
    let cols = [
        [c(0), c(3), c(6), 0.0],
        [c(1), c(4), c(7), 0.0],
        [c(2), c(5), c(8), 0.0],
        [c(9), c(10), c(11), 1.0],
    ];
    // sum(r*c for r, c in zip(row, col)), left to right.
    let dot = |r: &[f64; 4], k: &[f64; 4]| {
        let mut s = 0.0;
        for i in 0..4 {
            s += r[i] * k[i];
        }
        s
    };
    let mut out = vec![0.0; 13];
    for (ri, row) in rows.iter().enumerate() {
        for ci in 0..3 {
            out[ri * 3 + ci] = dot(row, &cols[ci]);
        }
        out[9 + ri] = dot(row, &cols[3]);
    }
    out[12] = p(12) * c(12);
    out
}

// ── attribute dictionaries ──────────────────────────────────────────────

/// `_decode_vff_attr_value`: one `A438`-wrapped value. An unknown type
/// tag (or a payload of the wrong size) decodes as null.
fn decode_attr_value(a438: &TlvNode) -> AttrValue {
    let Some(child) = a438.children.first() else {
        return AttrValue::Null;
    };
    let p = child.payload;
    match child.tag {
        AD38 => AttrValue::Str(py::utf8_replace(p)),
        AF38 | A938 if p.len() == 8 => AttrValue::Float(f64_at(p, 0)),
        A738 if p.len() == 4 => {
            AttrValue::Int(i64::from(i32::from_le_bytes([p[0], p[1], p[2], p[3]])))
        }
        B438 | B538 if p.len() == 24 => AttrValue::Triple(vec3_at(p, 0)),
        AE38 => AttrValue::List(child.children.iter().map(decode_attr_value).collect()),
        _ => AttrValue::Null,
    }
}

/// `extract_attribute_dictionaries`: every attribute dictionary under a
/// `D007`, by its declared name (`B436`), each the `B636` key / `A438`
/// value pairs of the sibling that follows the name.
pub fn extract_attribute_dictionaries(d007: &TlvNode) -> R<AttrDicts> {
    let Some(dc05) = direct(&d007.children, DC05) else {
        return Ok(IndexMap::new());
    };
    let elements = parse_tlv_recursive(dc05.payload, 0, dc05.payload.len(), &PROP_CONTAINER_TAGS)
        .map_err(|e| Fault(e.to_string()))?;
    let mut dicts = IndexMap::new();
    walk_dicts(&elements, &mut dicts);
    Ok(dicts)
}

fn walk_dicts(nodes: &[TlvNode], dicts: &mut AttrDicts) {
    for (i, n) in nodes.iter().enumerate() {
        if n.tag == B436 {
            let name = py::utf8_replace(n.payload);
            let mut entries = IndexMap::new();
            if let Some(next) = nodes.get(i + 1) {
                extract_entries(&next.children, &mut entries);
            }
            dicts.insert(name, entries);
        } else {
            walk_dicts(&n.children, dicts);
        }
    }
}

fn extract_entries(nodes: &[TlvNode], entries: &mut IndexMap<String, AttrValue>) {
    let mut key: Option<String> = None;
    for n in nodes {
        if n.tag == B636 {
            key = Some(py::utf8_replace(n.payload));
        } else if n.tag == A438 && key.is_some() {
            if let Some(k) = key.take() {
                entries.insert(k, decode_attr_value(n));
            }
        } else {
            extract_entries(&n.children, entries);
        }
    }
}

/// `extract_dynamic_properties`: the `dynamic_attributes` dictionary,
/// stringified.
pub fn extract_dynamic_properties(d007: &TlvNode) -> R<IndexMap<String, String>> {
    Ok(extract_attribute_dictionaries(d007)?
        .get("dynamic_attributes")
        .map(|d| d.iter().map(|(k, v)| (k.clone(), v.stringify())).collect())
        .unwrap_or_default())
}

// ── scenes and dimensions ───────────────────────────────────────────────

/// TLV records as `(tag, payload)`.
type TlvItems<'a> = Vec<(u16, &'a [u8])>;

/// `_tlv_items`: `buf` as a run of `(u16 tag, u32 len, payload)` records
/// covering the whole buffer, or `None` when it does not fit.
fn tlv_items(buf: &[u8]) -> Option<TlvItems<'_>> {
    let mut items = Vec::new();
    let mut off = 0usize;
    while off < buf.len() {
        if off + 6 > buf.len() {
            return None;
        }
        let tag = u16::from_le_bytes([buf[off], buf[off + 1]]);
        let ln = tlv::read_u32(buf, off + 2)? as usize;
        if tag == 0 || off + 6 + ln > buf.len() {
            return None;
        }
        items.push((tag, &buf[off + 6..off + 6 + ln]));
        off += 6 + ln;
    }
    Some(items)
}

/// `_tlv_find`.
fn tlv_find<'a>(items: &[(u16, &'a [u8])], tag: u16) -> Option<&'a [u8]> {
    items.iter().find(|(t, _)| *t == tag).map(|(_, p)| *p)
}

/// `_scan_vertex_positions`: every `C409` vertex's persistent id → its
/// position, for dimension references.
fn scan_vertex_positions(top: &TlvNode, id2pos: &mut HashMap<Vec<u8>, Point>) -> R<()> {
    if top.tag == C409 {
        let dc05 = find_child_tag(&top.children, DC05);
        let c509 = find_child_tag(&top.children, C509);
        if let (Some(dc05), Some(c509)) = (dc05, c509) {
            if c509.payload.len() == 24 {
                let id = dc05_id_bytes(dc05.payload)?;
                id2pos.insert(id.to_vec(), vec3_at(c509.payload, 0));
            }
        }
    }
    for c in &top.children {
        scan_vertex_positions(c, id2pos)?;
    }
    Ok(())
}

/// `_scan_instance_transforms`: every instance's persistent id → its
/// world transform, composing parent × local down the instance tree.
fn scan_instance_transforms(
    nodes: &[TlvNode],
    parent: &[f64],
    world: &mut HashMap<Vec<u8>, Vec<f64>>,
) -> R<()> {
    for el in nodes {
        if el.tag == T6419 {
            let dc05 =
                find_child_tag(&el.children, D007).and_then(|d| find_child_tag(&d.children, DC05));
            let iid = match dc05 {
                Some(dc05) => Some(dc05_id_bytes(dc05.payload)?),
                None => None,
            };
            let mat: Option<Vec<f64>> = find_child_tag(&el.children, T6619)
                .filter(|m| m.payload.len() == 104)
                .map(|m| (0..13).map(|i| f64_at(m.payload, i * 8)).collect());
            let here = match &mat {
                Some(m) => multiply_matrices(parent, m),
                None => parent.to_vec(),
            };
            if let Some(iid) = iid.filter(|i| !i.is_empty()) {
                world.insert(iid.to_vec(), here.clone());
            }
            scan_instance_transforms(&el.children, &here, world)?;
        } else if !el.children.is_empty() {
            scan_instance_transforms(&el.children, parent, world)?;
        }
    }
    Ok(())
}

/// `_parse_dimensions`: every linear dimension (`5BCC` record) in
/// `model_dat`, endpoints in world space. A connection point that cannot
/// be resolved drops the dimension.
pub fn parse_dimensions(
    model_dat: &[u8],
    id2pos: &HashMap<Vec<u8>, Point>,
    inst_world: &HashMap<Vec<u8>, Vec<f64>>,
) -> Vec<RawDimension> {
    let point = |block: &[u8]| -> Option<Point> {
        let blk = tlv_find(&tlv_items(block).unwrap_or_default(), 0x5208)?;
        let sub = tlv_items(blk).unwrap_or_default();
        let typ = tlv_find(&sub, 0x5209)
            .filter(|t| t.len() == 4)
            .map(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]]));
        if typ == Some(1) {
            let pos = tlv_find(&sub, 0x520A).filter(|p| p.len() == 24)?;
            return Some(vec3_at(pos, 0));
        }
        // Type 2: a vertex by persistent id, inside an instance.
        let f53fc = tlv_find(&sub, 0x520B)
            .and_then(|r| tlv_find(&tlv_items(r).unwrap_or_default(), 0x53FC));
        let fi = f53fc.and_then(tlv_items).unwrap_or_default();
        let vid = tlv_find(&fi, 0x53FD).filter(|v| !v.is_empty())?;
        let iid = tlv_find(&fi, 0x53FE);
        let local = *id2pos.get(vid)?;
        if let Some(iid) = iid {
            let n = iid.first().copied().unwrap_or(0) as usize;
            if n > 0 && n < iid.len() {
                if let Some(w) = inst_world.get(&iid[1..1 + n]).filter(|w| !w.is_empty()) {
                    return Some(transform_point(local, w));
                }
            }
        }
        Some(local)
    };
    let vec3 = |p: Option<&[u8]>| p.filter(|p| p.len() == 24).map(|p| vec3_at(p, 0));

    let mut dims = Vec::new();
    let n = model_dat.len();
    let mut i = 0;
    while let Some(j) = find(model_dat, b"\xcc\x5b", i) {
        i = j + 1;
        if j + 6 > n {
            continue;
        }
        let ln = tlv::read_u32(model_dat, j + 2).unwrap_or(0) as usize;
        if ln < 40 || j + 6 + ln > n {
            continue;
        }
        let Some(body) = tlv_items(&model_dat[j + 6..j + 6 + ln]) else {
            continue;
        };
        let (Some(pa), Some(pb)) = (tlv_find(&body, 0x5BCD), tlv_find(&body, 0x5BCE)) else {
            continue;
        };
        let (Some(a), Some(b)) = (point(pa), point(pb)) else {
            continue;
        };
        let off = tlv_find(&body, 0x5BD2).filter(|o| o.len() == 8);
        dims.push(RawDimension {
            a: Some(a),
            b: Some(b),
            plane_x: vec3(tlv_find(&body, 0x5BCF)),
            normal: vec3(tlv_find(&body, 0x5BD0)),
            offset: off.map_or(0.0, |o| f64_at(o, 0)),
            ..Default::default()
        });
    }
    dims
}

/// `bytes.find(needle, from)`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// `_find_page_node`: the payload of the `0702` scenes node in `top`.
fn find_page_node<'a>(top: &TlvNode<'a>) -> Option<&'a [u8]> {
    if top.tag == T0702 {
        return Some(top.payload);
    }
    top.children.iter().find_map(find_page_node)
}

/// `_tlv_items` of a record that must exist: Python calls `len(None)` on
/// a missing one, and the `TypeError` fails the whole parse.
fn required_items<'a>(p: Option<&'a [u8]>, what: &str) -> R<Option<TlvItems<'a>>> {
    match p {
        Some(p) => Ok(tlv_items(p)),
        None => Err(Fault(format!("scene record without {what}"))),
    }
}

/// `_parse_pages`: the saved scenes in a `0702` payload (`6D60 > 6D61 >`
/// one `7148` per page). A page without eye and target is dropped.
pub fn parse_pages(payload: Option<&[u8]>) -> R<Vec<RawPage>> {
    let Some(payload) = payload else {
        return Ok(Vec::new());
    };
    let vec3 = |p: Option<&[u8]>| p.filter(|p| p.len() == 24).map(|p| vec3_at(p, 0));
    let mut pages = Vec::new();
    for (t60, p60) in tlv_items(payload).unwrap_or_default() {
        if t60 != 0x6D60 {
            continue;
        }
        for (t61, p61) in tlv_items(p60).unwrap_or_default() {
            if t61 != 0x6D61 {
                continue;
            }
            for (t48, p48) in tlv_items(p61).unwrap_or_default() {
                if t48 != 0x7148 {
                    continue;
                }
                let Some(items) = tlv_items(p48) else {
                    continue;
                };
                let mut page = RawPage::default();
                let head = required_items(tlv_find(&items, 0x6F54), "6F54")?;
                if let Some(name) = head.as_deref().and_then(|h| tlv_find(h, 0x6F55)) {
                    if !name.is_empty() {
                        page.name = py::utf8_replace(name);
                    }
                }
                let cam_wrap = required_items(tlv_find(&items, 0x714A), "714A")?;
                let cam = match cam_wrap.as_deref() {
                    Some(w) if !w.is_empty() => required_items(tlv_find(w, 0x34BC), "34BC")?,
                    _ => None,
                };
                if let Some(cam) = cam.filter(|c| !c.is_empty()) {
                    page.eye = vec3(tlv_find(&cam, 0x34BD));
                    page.target = vec3(tlv_find(&cam, 0x34BE));
                    page.up = vec3(tlv_find(&cam, 0x34BF));
                    if let Some(fov) = tlv_find(&cam, 0x34C4).filter(|f| f.len() == 8) {
                        page.fov = f64_at(fov, 0);
                    }
                    // 34C2 = PERSPECTIVE flag: 00 means parallel projection.
                    let flag = tlv_find(&cam, 0x34C2);
                    page.parallel = flag.is_some_and(|f| f.first() == Some(&0));
                    if let Some(h) = tlv_find(&cam, 0x34C3).filter(|h| h.len() == 8) {
                        page.ortho_height = f64_at(h, 0);
                    }
                }
                // 7150 = hidden layers: (u8 length, var-int id) runs.
                if let Some(hidden) = tlv_find(&items, 0x7150) {
                    let mut off = 0usize;
                    while off < hidden.len() {
                        let ln = hidden[off] as usize;
                        if ln == 0 || off + 1 + ln > hidden.len() {
                            break;
                        }
                        page.hidden_layer_ids.push(var_int(hidden, off + 1, ln)?);
                        off += 1 + ln;
                    }
                }
                if page.eye.is_some() && page.target.is_some() {
                    pages.push(page);
                }
            }
        }
    }
    Ok(pages)
}

// ── the walk ────────────────────────────────────────────────────────────

/// What `full_parse` gathers while walking the top-level records.
#[derive(Default)]
struct Walk {
    layer_id_to_name: IndexMap<i64, String>,
    layer_hidden: IndexMap<String, bool>,
    material_id_to_name: IndexMap<i64, String>,
    defs: IndexMap<Option<i64>, RawDefinition>,
    root: GeometryBuilder,
    vertex_positions: HashMap<Vec<u8>, Point>,
    instance_world: HashMap<Vec<u8>, Vec<f64>>,
}

impl Walk {
    /// `collect_layers`: the layer manager's `993A > 8C3C` records.
    fn collect_layers(&mut self, nodes: &[TlvNode]) -> R<()> {
        for el in nodes {
            if el.tag == T993A {
                for child in el.children.iter().filter(|c| c.tag == T8C3C) {
                    let dc05 = find_child_tag(&child.children, DC05);
                    let name_node = find_child_tag(&child.children, T8D3C);
                    if let (Some(dc05), Some(name_node)) = (dc05, name_node) {
                        let l_id = dc05_id(dc05.payload)?;
                        let l_name = py::utf8_replace(name_node.payload);
                        self.layer_id_to_name.insert(l_id, l_name.clone());
                        // 8E3C: one byte, 1 = hidden.
                        if let Some(&h) =
                            find_child_tag(&child.children, T8E3C).and_then(|n| n.payload.first())
                        {
                            self.layer_hidden.insert(l_name, h == 1);
                        }
                    }
                }
            }
            self.collect_layers(&el.children)?;
        }
        Ok(())
    }

    /// `collect_material_ids`: `C832` records, TLV material id → name.
    fn collect_material_ids(&mut self, nodes: &[TlvNode]) -> R<()> {
        for el in nodes {
            if el.tag == C832 {
                let dc05 = find_child_tag(&el.children, DC05);
                let name_node = find_child_tag(&el.children, CC32);
                if let (Some(dc05), Some(name_node)) = (dc05, name_node) {
                    let m_id = dc05_id(dc05.payload)?;
                    self.material_id_to_name
                        .insert(m_id, py::utf8_replace(name_node.payload));
                }
            }
            self.collect_material_ids(&el.children)?;
        }
        Ok(())
    }

    /// `collect_defs`: every `7C15` component definition.
    fn collect_defs(&mut self, nodes: &[TlvNode]) -> R<()> {
        for el in nodes {
            if el.tag == T7C15 {
                let mut d = RawDefinition::default();
                for child in &el.children {
                    match child.tag {
                        T7D15 if child.payload.len() == 16 => {
                            d.guid = Some(py::hex_upper(child.payload));
                        }
                        T7E15 => d.name = Some(py::utf8_replace(child.payload)),
                        // Behaviour flags: 5D1B == 1 "always faces camera",
                        // 5E1B == 1 "shadows face sun".
                        T581B => {
                            for (tag, body) in tlv_flat(child.payload) {
                                if body.is_empty() {
                                    continue;
                                }
                                if tag == T5D1B {
                                    d.always_faces_camera = var_int_all(body) == 1;
                                } else if tag == T5E1B {
                                    d.shadows_face_sun = var_int_all(body) == 1;
                                }
                            }
                        }
                        // Definition kind: 2 backs an Image entity.
                        T8315 if !child.payload.is_empty() => {
                            d.is_image = var_int_all(child.payload) == 2;
                        }
                        _ => {}
                    }
                }
                let ent_id = extract_entity_id(el)?;
                extract_geometry_from_nodes(&el.children, &mut d.builder)?;
                self.defs.insert(ent_id, d);
            }
            self.collect_defs(&el.children)?;
        }
        Ok(())
    }

    /// Everything `full_parse` does with one top-level record.
    fn record(&mut self, el: &TlvNode) -> R<()> {
        let one = std::slice::from_ref(el);
        self.collect_layers(one)?;
        self.collect_material_ids(one)?;
        self.collect_defs(one)?;
        scan_vertex_positions(el, &mut self.vertex_positions)?;
        scan_instance_transforms(one, &[], &mut self.instance_world)?;
        if el.tag == F601 {
            extract_geometry_from_nodes(&el.children, &mut self.root)?;
        }
        Ok(())
    }
}

/// `full_parse` for a VFF container: materials and layer colours from the
/// material XMLs, the thumbnail, units, styles, then `model.dat`.
pub fn full_parse(mut zf: VffArchive) -> Result<Parsed, SkpParseError> {
    let names = zf.names();
    let mut parsed = Parsed {
        version: zf.version.clone(),
        ..Default::default()
    };

    // Layer colours come from `Layer_<name>` materials, which carry no
    // visibility; that comes from the layer manager (`collect_layers`).
    for name in &names {
        if !(name.ends_with("material.xml") && name.starts_with("materials/")) {
            continue;
        }
        let xml_data = zf.read(name)?;
        let Some(mat) = xml::parse_material(&mut zf, &names, name, &xml_data) else {
            continue;
        };
        let folder = name.split('/').nth(1).unwrap_or_default().to_string();
        if let Some(layer) = mat.name.strip_prefix("Layer_") {
            parsed.layer_colors.insert(
                layer.to_string(),
                (mat.color[0], mat.color[1], mat.color[2]),
            );
            parsed.layer_hidden.insert(layer.to_string(), false);
        }
        let mat_name = mat.name.clone();
        parsed.material_data.push(mat);
        let ix = parsed.material_data.len() - 1;
        parsed.materials.insert(mat_name, ix);
        if !folder.is_empty() {
            parsed.materials_by_folder.insert(folder, ix);
        }
    }

    // The thumbnail is read (and so size-checked) though nothing keeps it.
    if names.iter().any(|n| n == "meta/model_thumbnail.png") {
        zf.read("meta/model_thumbnail.png")?;
    }
    parsed.units = zf.units();

    for name in &names {
        if !(name.starts_with("styles/") && name.ends_with("style.xml")) {
            continue;
        }
        let data = zf.read(name)?;
        if let Some(style) = xml::parse_style(&data) {
            parsed.styles.push(style);
        }
    }

    let model_dat = zf.read("model.dat")?;
    drop(zf);

    let mut walk = Walk::default();
    let mut page_node: Option<&[u8]> = None;
    for item in iter_top_level(&model_dat, 0, model_dat.len(), &CONTAINER_TAGS) {
        let (index, total, el) = item?;
        walk.record(&el).map_err(|e| {
            SkpParseError::new(
                format!("Failed while processing top-level record: {e}"),
                Stage::TlvWalk,
            )
            .at_record(index, total)
            .with_tag(el.tag)
            .caused_by(e)
        })?;
        if page_node.is_none() {
            page_node = find_page_node(&el);
        }
    }

    walk.layer_id_to_name
        .entry(1)
        .or_insert_with(|| "Layer0".into());
    parsed
        .layer_colors
        .entry("Layer0".into())
        .or_insert((136, 136, 136));
    for (name, hidden) in walk.layer_hidden {
        parsed.layer_hidden.insert(name, hidden);
    }
    parsed.layer_hidden.entry("Layer0".into()).or_insert(false);

    parsed.pages = parse_pages(page_node).map_err(|e| {
        SkpParseError::new(format!("Failed while parsing scenes: {e}"), Stage::TlvWalk).caused_by(e)
    })?;
    parsed.dimensions = parse_dimensions(&model_dat, &walk.vertex_positions, &walk.instance_world);
    parsed.layer_id_to_name = walk.layer_id_to_name;
    parsed.material_id_to_name = walk.material_id_to_name;
    parsed.defs = walk.defs;
    parsed.root = Some(root_definition(walk.root));
    Ok(parsed)
}
