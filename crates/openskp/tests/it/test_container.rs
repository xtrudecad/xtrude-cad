// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The container layer on synthetic files: header magic, version string,
//! legacy routing, ZIP location, `meta/meta.dat` units, F401/F901
//! unwrapping and the depth guard.

use std::io::{Cursor, Write};

use openskp::container::{self, read_meta_units, read_version, Container};
use openskp::tlv::{iter_top_level, parse_tlv_recursive, Tag, CONTAINER_TAGS, MAX_TLV_DEPTH};
use openskp::Stage;

fn tlv(tag: [u8; 2], payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A VFF-shaped file: magic, a version marker, then a ZIP of `entries`.
fn vff_file(version: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = container::MAGIC.to_vec();
    out.extend(utf16("SketchUp Model"));
    out.extend(b"\xFF\xFE\xFF\x0E");
    out.extend(utf16(version));
    let mut zip = Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut zip);
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
    out.extend(zip.into_inner());
    out
}

#[test]
fn bad_magic_is_a_header_error() {
    let err = container::open(b"not a sketchup file".to_vec())
        .err()
        .unwrap();
    assert_eq!(err.stage, Some(Stage::Header));
    assert_eq!(
        err.to_string(),
        "Not a valid SketchUp file (bad header magic) | stage=header"
    );
}

#[test]
fn magic_without_zip_or_legacy_marker_has_no_zip_container() {
    let mut data = container::MAGIC.to_vec();
    data.extend([0u8; 600]);
    let err = container::open(data).err().unwrap();
    assert_eq!(err.stage, Some(Stage::ZipExtract));
    assert!(err.message.contains("No ZIP container found"));
}

#[test]
fn classic_file_routes_to_the_legacy_walker() {
    let mut data = container::MAGIC.to_vec();
    data.extend(b"\x00\x00CVersionMap");
    data.extend([0u8; 64]);
    assert!(matches!(
        container::open(data),
        Ok(Container::Legacy(d)) if d.starts_with(&container::MAGIC)
    ));
}

#[test]
fn version_is_the_braced_utf16_string() {
    let file = vff_file("{24.0.484}", &[("model.dat", b"")]);
    assert_eq!(read_version(&file), "{24.0.484}");
    let Ok(Container::Vff(archive)) = container::open(file) else {
        panic!("expected a VFF container");
    };
    assert_eq!(archive.version, "{24.0.484}");
}

#[test]
fn version_without_braces_is_unknown() {
    let file = vff_file("24.0.484", &[("model.dat", b"")]);
    assert_eq!(read_version(&file), "unknown");
}

#[test]
fn archive_lists_entries_in_order_and_reads_them() {
    let file = vff_file(
        "{24.0.484}",
        &[
            ("model.dat", b"abc"),
            ("materials/a/material.xml", b"<x/>"),
            ("meta/model_thumbnail.png", b""),
        ],
    );
    let Ok(Container::Vff(mut archive)) = container::open(file) else {
        panic!("expected a VFF container");
    };
    assert_eq!(
        archive.names(),
        [
            "model.dat",
            "materials/a/material.xml",
            "meta/model_thumbnail.png"
        ]
    );
    assert_eq!(archive.read("model.dat").unwrap(), b"abc");
    assert!(!archive.contains("meta/meta.dat"));
    assert_eq!(archive.units(), None);
    let missing = archive.read("nope").unwrap_err();
    assert_eq!(missing.stage, Some(Stage::ZipExtract));
}

#[test]
fn units_come_from_the_wrapped_meta_record() {
    let mut inner = tlv([0x65, 0x00], b"24.0");
    inner.extend(tlv([0x6D, 0x00], b"Millimeter"));
    let meta = tlv([0x64, 0x00], &inner);
    assert_eq!(read_meta_units(&meta).as_deref(), Some("Millimeter"));

    let file = vff_file("{24.0.484}", &[("meta/meta.dat", &meta)]);
    let Ok(Container::Vff(mut archive)) = container::open(file) else {
        panic!("expected a VFF container");
    };
    assert_eq!(archive.units().as_deref(), Some("Millimeter"));
}

#[test]
fn truncated_meta_has_no_units() {
    let mut meta = tlv([0x6D, 0x00], b"Millimeter");
    meta.truncate(meta.len() - 1);
    assert_eq!(read_meta_units(&meta), None);
}

#[test]
fn top_level_unwraps_a_lone_f401_and_the_f901_definition_list() {
    let def_a = tlv([0x7C, 0x15], &tlv([0x01, 0x00], b"a"));
    let def_b = tlv([0x7C, 0x15], &tlv([0x01, 0x00], b"b"));
    let list = tlv([0x71, 0x17], &[def_a, def_b].concat());
    let f901 = tlv([0xF9, 0x01], &tlv([0x70, 0x17], &list));
    let layers = tlv([0xD4, 0x30], &tlv([0x02, 0x00], b"l"));
    let data = tlv([0xF4, 0x01], &[layers, f901].concat());

    let it = iter_top_level(&data, 0, data.len(), &CONTAINER_TAGS);
    assert_eq!(it.total(), 3);
    let got: Vec<(usize, usize, Tag)> = it
        .map(|r| r.unwrap())
        .map(|(i, n, node)| (i, n, node.tag))
        .collect();
    assert_eq!(
        got,
        [
            (0, 3, Tag::new(0xD4, 0x30)),
            (1, 3, Tag::new(0x7C, 0x15)),
            (2, 3, Tag::new(0x7C, 0x15)),
        ]
    );
}

#[test]
fn f901_of_another_shape_stays_one_record() {
    let f901 = tlv([0xF9, 0x01], &tlv([0x01, 0x00], b"x"));
    let it = iter_top_level(&f901, 0, f901.len(), &CONTAINER_TAGS);
    assert_eq!(it.total(), 1);
}

#[test]
fn nesting_past_the_depth_limit_is_a_tlv_walk_error() {
    let mut data = tlv([0x01, 0x00], b"leaf");
    for _ in 0..=MAX_TLV_DEPTH + 1 {
        data = tlv([0x7C, 0x15], &data);
    }
    let err = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap_err();
    assert_eq!(err.stage, Some(Stage::TlvWalk));

    let top: Vec<_> = iter_top_level(&data, 0, data.len(), &CONTAINER_TAGS).collect();
    let err = top[0].as_ref().err().unwrap();
    assert_eq!((err.record_index, err.total_records), (Some(0), Some(1)));
    assert_eq!(err.tag, Some(Tag::new(0x7C, 0x15)));
}

#[test]
fn range_past_the_buffer_is_clamped() {
    let data = tlv([0x01, 0x00], b"ab");
    let nodes = parse_tlv_recursive(&data, 0, data.len() + 100, &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 1);
}
