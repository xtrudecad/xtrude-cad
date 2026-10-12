// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP `packages/python/tests/test_edge_surfacing.py`: the
//! per-element tables the walkers surface (edge layers, `Edge#curve`,
//! classic arc frames, face layers).

use crate::common;

use std::collections::{HashMap, HashSet};

use common::fixture;
use openskp::parsed::{ArcParams, GeometryBuilder};
use openskp::tlv::{Tag, TlvNode};
use openskp::vff::extract_geometry_from_nodes;
use openskp::Point;

/// Every definition's builder in the file, the root's last.
fn builders(name: &str) -> Vec<GeometryBuilder> {
    let parsed = openskp::full_parse(fixture(name)).unwrap();
    parsed
        .defs
        .into_values()
        .map(|d| d.builder)
        .chain(parsed.root.map(|r| r.builder))
        .collect()
}

/// One table merged across every definition (`dict.update`).
fn table<V: Clone>(name: &str, get: fn(&GeometryBuilder) -> Vec<(i64, V)>) -> HashMap<i64, V> {
    let mut merged = HashMap::new();
    for b in builders(name) {
        merged.extend(get(&b));
    }
    merged
}

fn edge_layers(b: &GeometryBuilder) -> Vec<(i64, i64)> {
    b.edge_layers.iter().map(|(&k, &v)| (k, v)).collect()
}

fn edge_curves(b: &GeometryBuilder) -> Vec<(i64, i64)> {
    b.edge_curves.iter().map(|(&k, &v)| (k, v)).collect()
}

fn arc_curves(b: &GeometryBuilder) -> Vec<(i64, ArcParams)> {
    b.arc_curves.iter().map(|(&k, v)| (k, v.clone())).collect()
}

fn face_layers(name: &str) -> HashMap<Option<i64>, usize> {
    let mut counts = HashMap::new();
    for b in builders(name) {
        for f in b.faces.values() {
            *counts.entry(f.layer).or_insert(0) += 1;
        }
    }
    counts
}

fn norm(v: Point) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dist(a: Point, b: Point) -> f64 {
    norm([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

// ── TestEdgeLayers ──

#[test]
fn test_a_single_layer_file_leaves_the_table_empty() {
    assert!(table("capilla_quiroz_v17.skp", edge_layers).is_empty());
}

// ── TestFaceLayers ──

#[test]
fn test_default_layer_zero_is_read_not_dropped() {
    assert_eq!(
        face_layers("capilla_quiroz_v17.skp"),
        HashMap::from([(Some(0), 181)])
    );
}

// ── TestCurveGrouping ──

#[test]
fn test_classic_curve_pointers_and_their_ids() {
    let curves = table("capilla_quiroz_v17.skp", edge_curves);
    assert_eq!(curves.len(), 39);
    assert!(curves.values().all(|&c| c > 0));
    let ids: HashSet<i64> = curves.values().copied().collect();
    assert_eq!(ids.len(), 6);
    let arcs: HashSet<i64> = table("capilla_quiroz_v17.skp", arc_curves)
        .into_keys()
        .collect();
    assert_eq!(ids, arcs);
}

// ── TestArcFrames ──

#[test]
fn test_frame_layout_is_the_files_own() {
    let frames: Vec<ArcParams> = table("capilla_quiroz_v17.skp", arc_curves)
        .into_values()
        .collect();
    assert_eq!(frames.len(), 6);
    for f in &frames {
        assert!((norm(f.normal) - 1.0).abs() <= 1e-9);
        assert!(dist(cross(f.normal, f.x_axis), f.y_axis) <= 1e-9);
        assert!((norm(f.x_axis) - 28.827831).abs() <= 1e-5);
        assert!((norm(f.x_axis) - norm(f.y_axis)).abs() <= 1e-5);
        assert!(f.start_angle.abs() <= 1e-12);
    }
    let ends: HashSet<i64> = frames
        .iter()
        .map(|f| (f.end_angle * 1e6).round() as i64)
        .collect();
    assert_eq!(ends, HashSet::from([829263, 837889]));
}

// ── TestVffParserSurfacing ──

#[test]
fn test_edge_curves_are_surfaced() {
    let curves = table("Untitled.skp", edge_curves);
    assert_eq!(curves.len(), 8832);
    assert_eq!(curves.values().copied().collect::<HashSet<_>>().len(), 368);
}

#[test]
fn test_faces_carry_a_layer_key() {
    assert_eq!(face_layers("Untitled.skp"), HashMap::from([(None, 1588)]));
}

#[test]
fn test_no_arc_frames_are_decoded() {
    assert!(table("Untitled.skp", arc_curves).is_empty());
}

// ── TestVffElementRecords ──

fn node(tag: u16, payload: &'static [u8], children: Vec<TlvNode<'static>>) -> TlvNode<'static> {
    TlvNode {
        offset: 0,
        tag: Tag::new((tag >> 8) as u8, tag as u8),
        size: payload.len(),
        children,
        payload,
    }
}

fn byte(v: u8) -> &'static [u8] {
    Box::leak(Box::new([v]))
}

fn d007(layer: u8) -> TlvNode<'static> {
    node(0xD007, b"", vec![node(0xD207, byte(layer), vec![])])
}

fn edge_node(e: u8, v1: u8, v2: u8, layer: Option<u8>, curve: Option<u8>) -> TlvNode<'static> {
    let mut kids = vec![
        node(0xDE05, byte(e), vec![]),
        node(0xB90B, byte(v1), vec![]),
        node(0xBA0B, byte(v2), vec![]),
    ];
    if let Some(l) = layer {
        kids.push(d007(l));
    }
    if let Some(c) = curve {
        kids.push(node(0xBB0B, byte(c), vec![]));
    }
    node(0xB80B, b"", kids)
}

fn face_node(f: u8, layer: Option<u8>) -> TlvNode<'static> {
    let mut kids = vec![node(0xDE05, byte(f), vec![])];
    if let Some(l) = layer {
        kids.push(d007(l));
    }
    node(0xAC0D, b"", kids)
}

fn extract(nodes: &[TlvNode]) -> GeometryBuilder {
    let mut builder = GeometryBuilder::default();
    extract_geometry_from_nodes(nodes, &mut builder).unwrap();
    builder
}

#[test]
fn test_an_edge_record_yields_its_layer_and_its_curve() {
    let b = extract(&[edge_node(7, 1, 2, Some(43), Some(17))]);
    assert_eq!(
        b.edges.into_iter().collect::<Vec<_>>(),
        [(7, (Some(1), Some(2)))]
    );
    assert_eq!(b.edge_layers.into_iter().collect::<Vec<_>>(), [(7, 43)]);
    assert_eq!(b.edge_curves.into_iter().collect::<Vec<_>>(), [(7, 17)]);
}

#[test]
fn test_an_edge_record_without_d207_stores_nothing() {
    let b = extract(&[edge_node(7, 1, 2, None, None)]);
    assert!(b.edge_layers.is_empty());
    assert!(b.edge_curves.is_empty());
}

#[test]
fn test_a_face_record_yields_its_layer() {
    assert_eq!(extract(&[face_node(9, Some(43))]).faces[&9].layer, Some(43));
}

#[test]
fn test_a_face_record_without_d207_reports_none() {
    assert_eq!(extract(&[face_node(9, None)]).faces[&9].layer, None);
}
