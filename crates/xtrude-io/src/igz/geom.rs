// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The binary geometry blocks of an IngeTrazo format-3 document.
//!
//! A big mesh block is `{"geom": {"member": "geom/<n>.bin", "arrays":
//! {...}, "face_attrs": [...], "edge_layers": [...]}}`. The member is one
//! raw blob of little-endian arrays, each at its own offset, described in
//! `arrays` as `[dtype, shape, offset]`:
//!
//! | array | dtype | shape | |
//! |---|---|---|---|
//! | `pos` | `<f4` | n × 3 | the distinct points, numbered by first appearance in JSON order |
//! | `edges` | `<i4` | m × 2 | indices into `pos` |
//! | `rings` | `<i4` | | every face's outer ring then holes, indices into `pos` |
//! | `ring_sizes` | `<i4` | | the points of each ring |
//! | `ring_counts` | `<i4` | one per face | 1 + its holes |
//! | `face_attr` | `<i4` | one per face | an index into `face_attrs`, −1 for none |
//! | `edge_flags` | `\|u1` | m | bit 0 soft, bit 1 hidden |
//! | `edge_curve` | `<i8` | m, optional | the curve id, −1 for none |
//! | `edge_layer` | `<i4` | m, optional | an index into `edge_layers`, −1 for none |
//!
//! Every block is checked before it is used, so a damaged or crafted
//! document fails with an error instead of building garbage.

use std::collections::HashMap;

use serde_json::Value;
use xtrude_core::math::DVec3;
use xtrude_core::mesh::EdgeFlags;

use crate::schema::{face_attrs_from_json, FaceRec, MeshRecords};

/// The archive members that hold geometry blocks.
pub(super) const GEOM_PREFIX: &str = "geom/";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Dtype {
    F4,
    I4,
    I8,
    U1,
}

impl Dtype {
    fn parse(s: &str) -> Option<Dtype> {
        Some(match s {
            "<f4" => Dtype::F4,
            "<i4" => Dtype::I4,
            "<i8" => Dtype::I8,
            "|u1" => Dtype::U1,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Dtype::F4 | Dtype::I4 => 4,
            Dtype::I8 => 8,
            Dtype::U1 => 1,
        }
    }
}

/// One array of the blob, its bounds checked.
struct Array<'a> {
    dtype: Dtype,
    shape: Vec<usize>,
    bytes: &'a [u8],
}

fn bad(why: &str) -> String {
    format!("damaged geometry block ({why})")
}

fn array<'a>(name: &str, spec: &Value, blob: &'a [u8]) -> Result<Array<'a>, String> {
    let fail = || bad(name);
    let [dtype, shape, offset] = spec.as_array().map(Vec::as_slice).ok_or_else(fail)? else {
        return Err(fail());
    };
    let dtype = dtype.as_str().and_then(Dtype::parse).ok_or_else(fail)?;
    let shape: Vec<usize> = shape
        .as_array()
        .ok_or_else(fail)?
        .iter()
        .map(|n| n.as_u64().and_then(|n| usize::try_from(n).ok()))
        .collect::<Option<_>>()
        .ok_or_else(fail)?;
    let offset = offset
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(fail)?;
    let size = shape
        .iter()
        .try_fold(dtype.size(), |acc, &n| acc.checked_mul(n))
        .ok_or_else(fail)?;
    let end = offset.checked_add(size).ok_or_else(fail)?;
    let bytes = blob.get(offset..end).ok_or_else(fail)?;
    Ok(Array {
        dtype,
        shape,
        bytes,
    })
}

/// The array `name`, which must be there with this dtype and as many
/// dimensions as `cols` (`None`: one dimension; `Some(k)`: rows of `k`).
fn typed<'a>(
    arrays: &HashMap<&str, Array<'a>>,
    name: &str,
    dtype: Dtype,
    cols: Option<usize>,
) -> Result<&'a [u8], String> {
    let a = arrays
        .get(name)
        .ok_or_else(|| bad(&format!("missing {name}")))?;
    let shape_ok = match cols {
        None => a.shape.len() == 1,
        Some(k) => a.shape.len() == 2 && a.shape[1] == k,
    };
    if a.dtype != dtype || !shape_ok {
        return Err(bad(name));
    }
    Ok(a.bytes)
}

fn optional<'a>(
    arrays: &HashMap<&str, Array<'a>>,
    name: &str,
    dtype: Dtype,
) -> Result<Option<&'a [u8]>, String> {
    if arrays.contains_key(name) {
        typed(arrays, name, dtype, None).map(Some)
    } else {
        Ok(None)
    }
}

fn f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn i32s(b: &[u8]) -> Vec<i32> {
    b.chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn i64s(b: &[u8]) -> Vec<i64> {
    b.chunks_exact(8)
        .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect()
}

/// `values` as indices below `n`, or `None` when one is out of range.
fn indices(values: &[i32], n: usize) -> Option<Vec<usize>> {
    values
        .iter()
        .map(|&i| usize::try_from(i).ok().filter(|&i| i < n))
        .collect()
}

/// `values` as optional indices below `n` (−1 for none).
fn optional_indices(values: &[i32], n: usize) -> Option<Vec<Option<usize>>> {
    values
        .iter()
        .map(|&i| match i {
            -1 => Some(None),
            _ => usize::try_from(i).ok().filter(|&i| i < n).map(Some),
        })
        .collect()
}

/// `values` as counts of at least one.
fn counts(values: &[i32]) -> Option<Vec<usize>> {
    values
        .iter()
        .map(|&n| usize::try_from(n).ok().filter(|&n| n >= 1))
        .collect()
}

/// The records of the geometry block `meta`, read from `members` (the
/// document's `geom/` members by name). A point that is not a number
/// leaves out the edges and faces touching it, counted as dropped.
pub(super) fn decode(
    meta: &Value,
    members: &HashMap<String, Vec<u8>>,
) -> Result<MeshRecords, String> {
    let member = meta
        .get("member")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("no member"))?;
    let blob = members
        .get(member)
        .ok_or_else(|| format!("the document lost its geometry block {member}"))?;
    let layout = meta
        .get("arrays")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("no layout"))?;
    let arrays: HashMap<&str, Array> = layout
        .iter()
        .map(|(name, spec)| Ok((name.as_str(), array(name, spec, blob)?)))
        .collect::<Result<_, String>>()?;

    let pos = f32s(typed(&arrays, "pos", Dtype::F4, Some(3))?);
    let edges = i32s(typed(&arrays, "edges", Dtype::I4, Some(2))?);
    let rings = i32s(typed(&arrays, "rings", Dtype::I4, None)?);
    let ring_sizes = i32s(typed(&arrays, "ring_sizes", Dtype::I4, None)?);
    let ring_counts = i32s(typed(&arrays, "ring_counts", Dtype::I4, None)?);
    let face_attr = i32s(typed(&arrays, "face_attr", Dtype::I4, None)?);
    let edge_flags = typed(&arrays, "edge_flags", Dtype::U1, None)?;
    let edge_curve = optional(&arrays, "edge_curve", Dtype::I8)?.map(i64s);
    let edge_layer = optional(&arrays, "edge_layer", Dtype::I4)?.map(i32s);

    let points: Vec<DVec3> = pos
        .chunks_exact(3)
        .map(|p| DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
        .collect();
    let nv = points.len();
    let ne = edges.len() / 2;
    let edges = indices(&edges, nv).ok_or_else(|| bad("edges"))?;
    let rings = indices(&rings, nv).ok_or_else(|| bad("rings"))?;
    let ring_sizes = counts(&ring_sizes).ok_or_else(|| bad("rings"))?;
    let ring_counts = counts(&ring_counts).ok_or_else(|| bad("rings"))?;
    if ring_sizes.iter().sum::<usize>() != rings.len()
        || ring_counts.iter().sum::<usize>() != ring_sizes.len()
    {
        return Err(bad("rings"));
    }

    let table: Vec<Option<_>> = match meta.get("face_attrs") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().map(face_attrs_from_json).collect(),
        Some(_) => return Err(bad("face_attrs")),
    };
    if face_attr.len() != ring_counts.len() {
        return Err(bad("face_attr"));
    }
    let face_attr = optional_indices(&face_attr, table.len()).ok_or_else(|| bad("face_attr"))?;

    let layer_names: Vec<&str> = match meta.get("edge_layers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()
            .ok_or_else(|| bad("edge_layers"))?,
        Some(_) => return Err(bad("edge_layers")),
    };
    if edge_flags.len() != ne {
        return Err(bad("edge_flags"));
    }
    if edge_curve.as_ref().is_some_and(|c| c.len() != ne) {
        return Err(bad("edge_curve"));
    }
    let edge_layer = match edge_layer {
        Some(l) if l.len() != ne => return Err(bad("edge_layer")),
        Some(l) => Some(optional_indices(&l, layer_names.len()).ok_or_else(|| bad("edge_layer"))?),
        None => None,
    };

    let finite = |p: DVec3| p.is_finite();
    let mut out = MeshRecords::default();
    for (i, pair) in edges.chunks_exact(2).enumerate() {
        let (a, b) = (points[pair[0]], points[pair[1]]);
        if !(finite(a) && finite(b)) {
            out.dropped += 1;
            continue;
        }
        let flags = EdgeFlags {
            soft: edge_flags[i] & 1 != 0,
            hidden: edge_flags[i] & 2 != 0,
            curve: edge_curve.as_ref().map(|c| c[i]).filter(|&c| c >= 0),
            layer: edge_layer
                .as_ref()
                .and_then(|l| l[i])
                .map(|k| layer_names[k])
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        out.edges.push((a, b, flags));
    }
    let mut next_point = 0;
    let mut next_ring = 0;
    for (f, &count) in ring_counts.iter().enumerate() {
        let mut loops: Vec<Vec<DVec3>> = Vec::with_capacity(count);
        for &size in &ring_sizes[next_ring..next_ring + count] {
            let ids = &rings[next_point..next_point + size];
            loops.push(ids.iter().map(|&k| points[k]).collect());
            next_point += size;
        }
        next_ring += count;
        if !loops.iter().flatten().all(|&p| finite(p)) {
            out.dropped += 1;
            continue;
        }
        let holes = loops.split_off(1);
        let attrs = face_attr[f].and_then(|k| table[k].clone());
        let face: FaceRec = (loops.remove(0), holes, attrs);
        out.faces.push(face);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A blob of `arrays` (name, dtype, shape, little-endian bytes) at
    /// 8-byte-aligned offsets, and its layout.
    fn block(arrays: &[(&str, &str, Vec<usize>, Vec<u8>)]) -> (Value, HashMap<String, Vec<u8>>) {
        let mut blob = Vec::new();
        let mut layout = serde_json::Map::new();
        for (name, dtype, shape, bytes) in arrays {
            layout.insert((*name).to_string(), json!([dtype, shape, blob.len()]));
            blob.extend_from_slice(bytes);
            blob.resize(blob.len().div_ceil(8) * 8, 0);
        }
        let meta = json!({"member": "geom/0.bin", "arrays": layout});
        (meta, HashMap::from([("geom/0.bin".to_string(), blob)]))
    }

    fn le_f32(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn le_i32(v: &[i32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn le_i64(v: &[i64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// A unit square face and one free edge from its corner, the edge soft,
    /// on a curve and on the layer "Guides", the face painted.
    fn square() -> Vec<(&'static str, &'static str, Vec<usize>, Vec<u8>)> {
        vec![
            (
                "pos",
                "<f4",
                vec![5, 3],
                le_f32(&[0., 0., 0., 2., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]),
            ),
            ("edges", "<i4", vec![1, 2], le_i32(&[0, 1])),
            ("rings", "<i4", vec![4], le_i32(&[0, 2, 3, 4])),
            ("ring_sizes", "<i4", vec![1], le_i32(&[4])),
            ("ring_counts", "<i4", vec![1], le_i32(&[1])),
            ("face_attr", "<i4", vec![1], le_i32(&[0])),
            ("edge_flags", "|u1", vec![1], vec![1]),
            ("edge_curve", "<i8", vec![1], le_i64(&[7])),
            ("edge_layer", "<i4", vec![1], le_i32(&[0])),
        ]
    }

    fn with_tables(mut meta: Value) -> Value {
        meta["face_attrs"] = json!([{"color": [0.5, 0.25, 1.0]}]);
        meta["edge_layers"] = json!(["Guides"]);
        meta
    }

    fn decode_arrays(
        arrays: Vec<(&str, &str, Vec<usize>, Vec<u8>)>,
    ) -> Result<MeshRecords, String> {
        let (meta, members) = block(&arrays);
        decode(&with_tables(meta), &members)
    }

    /// `square()` with the array `name` replaced.
    fn replaced(
        name: &str,
        dtype: &'static str,
        shape: Vec<usize>,
        bytes: Vec<u8>,
    ) -> Result<MeshRecords, String> {
        let arrays = square()
            .into_iter()
            .map(|a| {
                if a.0 == name {
                    (a.0, dtype, shape.clone(), bytes.clone())
                } else {
                    a
                }
            })
            .collect();
        decode_arrays(arrays)
    }

    fn without(name: &str) -> Result<MeshRecords, String> {
        decode_arrays(square().into_iter().filter(|a| a.0 != name).collect())
    }

    fn assert_damaged(r: Result<MeshRecords, String>, why: &str) {
        match r {
            Err(e) => assert_eq!(e, format!("damaged geometry block ({why})")),
            Ok(_) => panic!("{why}: accepted"),
        }
    }

    #[test]
    fn test_a_valid_block_gives_its_records() {
        let rec = decode_arrays(square()).expect("valid");
        assert_eq!(rec.dropped, 0);
        assert_eq!(rec.edges.len(), 1);
        let (a, b, flags) = &rec.edges[0];
        assert_eq!((*a, *b), (DVec3::ZERO, DVec3::new(2., 0., 0.)));
        assert!(flags.soft && !flags.hidden);
        assert_eq!(flags.curve, Some(7));
        assert_eq!(flags.layer.as_deref(), Some("Guides"));
        let (outer, holes, attrs) = &rec.faces[0];
        assert_eq!(outer.len(), 4);
        assert_eq!(outer[1], DVec3::new(1., 0., 0.));
        assert!(holes.is_empty());
        assert_eq!(attrs.as_ref().unwrap().paint.color, Some([0.5, 0.25, 1.0]));
    }

    #[test]
    fn test_none_markers_read_as_none() {
        let rec = replaced("face_attr", "<i4", vec![1], le_i32(&[-1])).unwrap();
        assert!(rec.faces[0].2.is_none());
        let rec = replaced("edge_curve", "<i8", vec![1], le_i64(&[-1])).unwrap();
        assert_eq!(rec.edges[0].2.curve, None);
        let rec = replaced("edge_layer", "<i4", vec![1], le_i32(&[-1])).unwrap();
        assert_eq!(rec.edges[0].2.layer, None);
        let rec = without("edge_curve").unwrap();
        assert_eq!(rec.edges[0].2.curve, None);
    }

    #[test]
    fn test_a_face_with_holes_takes_its_rings_in_order() {
        let pos: Vec<f32> = [
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [0., 4.],
            [1., 1.],
            [2., 1.],
            [2., 2.],
            [1., 2.],
        ]
        .iter()
        .flat_map(|p| [p[0], p[1], 0.])
        .collect();
        let arrays = vec![
            ("pos", "<f4", vec![8, 3], le_f32(&pos)),
            ("edges", "<i4", vec![0, 2], Vec::new()),
            ("rings", "<i4", vec![8], le_i32(&[0, 1, 2, 3, 4, 5, 6, 7])),
            ("ring_sizes", "<i4", vec![2], le_i32(&[4, 4])),
            ("ring_counts", "<i4", vec![1], le_i32(&[2])),
            ("face_attr", "<i4", vec![1], le_i32(&[-1])),
            ("edge_flags", "|u1", vec![0], Vec::new()),
        ];
        let rec = decode_arrays(arrays).unwrap();
        let (outer, holes, _) = &rec.faces[0];
        assert_eq!(outer[2], DVec3::new(4., 4., 0.));
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0][0], DVec3::new(1., 1., 0.));
    }

    #[test]
    fn test_a_point_not_a_number_drops_what_touches_it() {
        let mut pos = vec![0., 0., 0., 2., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.];
        pos[0] = f32::NAN;
        let rec = replaced("pos", "<f4", vec![5, 3], le_f32(&pos)).unwrap();
        assert!(rec.edges.is_empty() && rec.faces.is_empty());
        assert_eq!(rec.dropped, 2);
    }

    #[test]
    fn test_unknown_dtypes_are_refused() {
        assert_damaged(replaced("pos", "<f8", vec![5, 3], vec![0; 120]), "pos");
        assert_damaged(replaced("edge_flags", "O", vec![1], vec![1]), "edge_flags");
    }

    #[test]
    fn test_an_array_of_the_wrong_dtype_or_shape_is_refused() {
        assert_damaged(replaced("pos", "<i4", vec![5, 3], vec![0; 60]), "pos");
        assert_damaged(replaced("pos", "<f4", vec![15], vec![0; 60]), "pos");
        assert_damaged(replaced("edges", "<i4", vec![2], le_i32(&[0, 1])), "edges");
        assert_damaged(
            replaced("rings", "<i4", vec![2, 2], le_i32(&[0, 2, 3, 4])),
            "rings",
        );
    }

    #[test]
    fn test_an_array_outside_the_blob_is_refused() {
        let (mut meta, members) = block(&square());
        meta["arrays"]["rings"][2] = json!(1_000_000);
        assert_damaged(decode(&with_tables(meta), &members), "rings");
        let (mut meta, members) = block(&square());
        meta["arrays"]["pos"][1] = json!([u64::MAX, 3]);
        assert_damaged(decode(&with_tables(meta), &members), "pos");
        let (mut meta, members) = block(&square());
        meta["arrays"]["pos"][2] = json!(-8);
        assert_damaged(decode(&with_tables(meta), &members), "pos");
        let (mut meta, members) = block(&square());
        meta["arrays"]["pos"] = json!(["<f4", [5, 3]]);
        assert_damaged(decode(&with_tables(meta), &members), "pos");
    }

    #[test]
    fn test_every_required_array_must_be_there() {
        for name in [
            "pos",
            "edges",
            "rings",
            "ring_sizes",
            "ring_counts",
            "face_attr",
            "edge_flags",
        ] {
            assert_damaged(without(name), &format!("missing {name}"));
        }
    }

    #[test]
    fn test_indices_out_of_range_are_refused() {
        assert_damaged(
            replaced("edges", "<i4", vec![1, 2], le_i32(&[0, 5])),
            "edges",
        );
        assert_damaged(
            replaced("edges", "<i4", vec![1, 2], le_i32(&[-1, 1])),
            "edges",
        );
        assert_damaged(
            replaced("rings", "<i4", vec![4], le_i32(&[0, 2, 3, 9])),
            "rings",
        );
    }

    #[test]
    fn test_ring_sizes_and_counts_must_add_up() {
        assert_damaged(
            replaced("ring_sizes", "<i4", vec![1], le_i32(&[3])),
            "rings",
        );
        assert_damaged(
            replaced("ring_sizes", "<i4", vec![2], le_i32(&[4, 0])),
            "rings",
        );
        assert_damaged(
            replaced("ring_counts", "<i4", vec![1], le_i32(&[2])),
            "rings",
        );
        assert_damaged(
            replaced("ring_counts", "<i4", vec![1], le_i32(&[0])),
            "rings",
        );
    }

    #[test]
    fn test_face_attributes_must_be_in_range() {
        assert_damaged(
            replaced("face_attr", "<i4", vec![1], le_i32(&[1])),
            "face_attr",
        );
        assert_damaged(
            replaced("face_attr", "<i4", vec![1], le_i32(&[-2])),
            "face_attr",
        );
        assert_damaged(
            replaced("face_attr", "<i4", vec![2], le_i32(&[0, 0])),
            "face_attr",
        );
        let (mut meta, members) = block(&square());
        meta = with_tables(meta);
        meta["face_attrs"] = json!({"color": [1, 0, 0]});
        assert_damaged(decode(&meta, &members), "face_attrs");
    }

    #[test]
    fn test_per_edge_arrays_must_have_the_edge_count() {
        assert_damaged(
            replaced("edge_flags", "|u1", vec![2], vec![0, 0]),
            "edge_flags",
        );
        assert_damaged(
            replaced("edge_curve", "<i8", vec![2], le_i64(&[1, 1])),
            "edge_curve",
        );
        assert_damaged(
            replaced("edge_layer", "<i4", vec![2], le_i32(&[0, 0])),
            "edge_layer",
        );
    }

    #[test]
    fn test_edge_layers_must_be_in_range_and_names() {
        assert_damaged(
            replaced("edge_layer", "<i4", vec![1], le_i32(&[1])),
            "edge_layer",
        );
        let (mut meta, members) = block(&square());
        meta = with_tables(meta);
        meta["edge_layers"] = json!([3]);
        assert_damaged(decode(&meta, &members), "edge_layers");
    }

    #[test]
    fn test_a_block_without_member_or_layout_is_refused() {
        let (meta, members) = block(&square());
        let mut no_member = with_tables(meta.clone());
        no_member.as_object_mut().unwrap().remove("member");
        assert_damaged(decode(&no_member, &members), "no member");
        let mut no_layout = with_tables(meta.clone());
        no_layout.as_object_mut().unwrap().remove("arrays");
        assert_damaged(decode(&no_layout, &members), "no layout");
        assert_eq!(
            decode(&with_tables(meta), &HashMap::new()).unwrap_err(),
            "the document lost its geometry block geom/0.bin"
        );
    }
}
