// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP `packages/python/tests/test_pages_dimensions.py`: VFF
//! scenes and linear dimensions, and the classic scene scan.

use crate::common;

use std::collections::HashMap;
use std::rc::Rc;

use common::fixture;
use openskp::legacy::{scan_pages, Archive, Obj, Slot, PAGE_HIDDEN_LAYERS_BIT, STR_MARKER};
use openskp::vff::{parse_dimensions, parse_pages};

/// One flat record: u16-LE tag, u32 length.
fn tlv(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_le_bytes().to_vec();
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn vec3(x: f64, y: f64, z: f64) -> Vec<u8> {
    [x, y, z].iter().flat_map(|v| v.to_le_bytes()).collect()
}

// ── linear dimensions ──

#[test]
fn test_untitled_fixture_has_13_dimensions() {
    let model = openskp::SkpFile::open(fixture("Untitled.skp"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(model.dimensions.len(), 13);
    for d in &model.dimensions {
        let (a, b) = (d.a.unwrap(), d.b.unwrap());
        let dist = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        assert!(dist > 0.0);
        assert!(d.normal.is_some() && d.plane_x.is_some());
    }
}

#[test]
fn test_dimension_free_points_synthetic() {
    let point_block = |wrap: u16, x: f64, y: f64, z: f64| {
        let inner = [
            tlv(0x5209, &1u32.to_le_bytes()),
            tlv(0x520A, &vec3(x, y, z)),
        ]
        .concat();
        tlv(wrap, &tlv(0x5208, &inner))
    };
    let body = [
        point_block(0x5BCD, 0.0, 0.0, 0.0),
        point_block(0x5BCE, 100.0, 0.0, 0.0),
        tlv(0x5BCF, &vec3(1.0, 0.0, 0.0)),
        tlv(0x5BD0, &vec3(0.0, 0.0, 1.0)),
        tlv(0x5BD2, &15.5f64.to_le_bytes()),
    ]
    .concat();
    let blob = [vec![0u8; 8], tlv(0x5BCC, &body), vec![0u8; 8]].concat();
    let dims = parse_dimensions(&blob, &HashMap::new(), &HashMap::new());
    assert_eq!(dims.len(), 1);
    let d = &dims[0];
    assert_eq!(d.a, Some([0.0, 0.0, 0.0]));
    assert_eq!(d.b, Some([100.0, 0.0, 0.0]));
    assert_eq!(d.offset, 15.5);
    assert_eq!(d.plane_x, Some([1.0, 0.0, 0.0]));
    assert_eq!(d.normal, Some([0.0, 0.0, 1.0]));
}

#[test]
fn test_dimension_connected_point_resolves_through_instance() {
    let vid = vec![0xaa, 0xbb, 0x01];
    let iid = vec![0xcc, 0xdd, 0x02];
    let connected = |wrap: u16| {
        let path = [vec![iid.len() as u8], iid.clone()].concat();
        let reference = tlv(0x53FC, &[tlv(0x53FD, &vid), tlv(0x53FE, &path)].concat());
        let inner = [tlv(0x5209, &2u32.to_le_bytes()), tlv(0x520B, &reference)].concat();
        tlv(wrap, &tlv(0x5208, &inner))
    };
    let free = |wrap: u16| {
        let inner = [
            tlv(0x5209, &1u32.to_le_bytes()),
            tlv(0x520A, &vec3(0.0, 0.0, 0.0)),
        ]
        .concat();
        tlv(wrap, &tlv(0x5208, &inner))
    };
    let body = [
        connected(0x5BCD),
        free(0x5BCE),
        tlv(0x5BD2, &0.0f64.to_le_bytes()),
    ]
    .concat();
    let blob = tlv(0x5BCC, &body);

    // A transform that translates by (10, 20, 30).
    let world = vec![
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 10.0, 20.0, 30.0, 1.0,
    ];
    let id2pos = HashMap::from([(vid.clone(), [1.0, 2.0, 3.0])]);
    let inst_world = HashMap::from([(iid.clone(), world)]);
    let dims = parse_dimensions(&blob, &id2pos, &inst_world);
    assert_eq!(dims.len(), 1);
    assert_eq!(dims[0].a, Some([11.0, 22.0, 33.0]));

    // An unknown vertex id drops the dimension.
    assert!(parse_dimensions(&blob, &HashMap::new(), &HashMap::new()).is_empty());
}

// ── scenes (pages) ──

fn page_record(name: &str, parallel: bool, hidden_ids: &[u8]) -> Vec<u8> {
    let cam = [
        tlv(0x34BD, &vec3(100.0, -200.0, 50.0)),
        tlv(0x34BE, &vec3(0.0, 0.0, 0.0)),
        tlv(0x34BF, &vec3(0.0, 0.0, 1.0)),
        tlv(0x34C4, &35.0f64.to_le_bytes()),
        tlv(0x34C2, &[if parallel { 0 } else { 1 }]),
        tlv(0x34C3, &240.0f64.to_le_bytes()),
    ]
    .concat();
    let hidden: Vec<u8> = hidden_ids.iter().flat_map(|&i| [1, i]).collect();
    let body = [
        tlv(0x6F54, &tlv(0x6F55, name.as_bytes())),
        tlv(0x714A, &tlv(0x34BC, &cam)),
        tlv(0x7150, &hidden),
    ]
    .concat();
    tlv(0x7148, &body)
}

#[test]
fn test_parse_pages_synthetic() {
    let records = [
        page_record("Planta", true, &[2]),
        page_record("Vista 3D", false, &[]),
    ]
    .concat();
    let payload = tlv(0x6D60, &tlv(0x6D61, &records));
    let pages = parse_pages(Some(&payload)).unwrap();
    let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Planta", "Vista 3D"]);
    let planta = &pages[0];
    assert!(planta.parallel);
    assert_eq!(planta.ortho_height, 240.0);
    assert_eq!(planta.eye, Some([100.0, -200.0, 50.0]));
    assert_eq!(planta.up, Some([0.0, 0.0, 1.0]));
    assert_eq!(planta.hidden_layer_ids, [2]);
    assert!(!pages[1].parallel);
    assert_eq!(pages[1].fov, 35.0);
}

#[test]
fn test_pages_absent_is_empty() {
    assert!(parse_pages(None).unwrap().is_empty());
    let model = openskp::SkpFile::open(fixture("SU_File.skp"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(model.pages.is_empty());
}

// ── classic pages (scenes) ──

fn legacy_page_record(name: &str, flags: u32, hidden_ids: &[u16]) -> Vec<u8> {
    let mut out = STR_MARKER.to_vec();
    out.push(name.len() as u8);
    out.extend(name.encode_utf16().flat_map(|u| u.to_le_bytes()));
    out.extend(STR_MARKER);
    out.push(0); // the second name is always empty
    out.extend(flags.to_le_bytes());
    out.extend([0, 0]); // camera: null ref
    out.extend([0, 0]); // thumbnail dib: null ref
    if flags & PAGE_HIDDEN_LAYERS_BIT != 0 {
        for id in hidden_ids {
            out.extend(id.to_le_bytes());
        }
        out.extend(1u32.to_le_bytes()); // the marker, which also ends the list
    }
    out
}

#[test]
fn test_scan_pages_legacy_synthetic() {
    let data = [
        legacy_page_record("Baseline", 0xc00, &[]),
        legacy_page_record("WithHidden", 0xc20, &[5, 6]),
        legacy_page_record("TooFull", 0xfff, &[5]),
    ]
    .concat();
    let mut ar = Archive::new(&data, 17);
    for (slot, name) in [(5, "Layer0"), (6, "Roof")] {
        ar.slots.insert(
            slot,
            Slot::Obj {
                name: Rc::from("CLayer"),
                value: Some(Rc::new(Obj::Layer {
                    name: name.into(),
                    hidden: 0,
                    rgba: [0; 4],
                })),
            },
        );
    }
    let pages = scan_pages(&data, &mut ar, 0).unwrap();
    let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Baseline", "WithHidden"]);
    assert!(pages[0].hidden_layer_ids.is_empty());
    assert_eq!(pages[1].hidden_layer_ids, [5, 6]);
}
