// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An IngeTrazo `.igz` that cannot be read is an error, never a panic: a
//! newer format, a ZIP container without its document, damaged bytes.

use crate::common;
use common::*;
use std::io::Write;
use std::path::Path;
use xtrude_io::igz::{self, IgzError, CURRENT_FORMAT};
use xtrude_model::scene::Scene;

fn load(path: &Path) -> Result<(), IgzError> {
    igz::load_into(&mut Scene::new(), path, None)
}

fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    out.into_inner()
}

#[test]
fn test_a_newer_igz_format_is_refused_with_its_number() {
    let tmp = TmpDir::new();
    let path = tmp.join("big.igz");
    let doc = serde_json::json!({"igz_format": CURRENT_FORMAT + 1, "scene": {}});
    std::fs::write(&path, doc.to_string()).unwrap();
    let err = load(&path).expect_err("refused");
    assert!(
        matches!(err, IgzError::TooNew(n) if n == CURRENT_FORMAT + 1),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "IngeTrazo document format v{} is newer than this build reads (v{CURRENT_FORMAT}).",
            CURRENT_FORMAT + 1
        )
    );
}

#[test]
fn test_a_zip_without_its_document_is_not_an_igz() {
    let tmp = TmpDir::new();
    let path = tmp.join("textures.igz");
    std::fs::write(&path, zip_with(&[("textures/wood.png", b"png")])).unwrap();
    assert!(matches!(load(&path), Err(IgzError::NotADocument(n)) if n == "textures.igz"));
}

#[test]
fn test_damaged_igz_files_are_errors() {
    let tmp = TmpDir::new();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        (
            "zip_magic_then_garbage",
            b"PK\x03\x04 not a zip at all".to_vec(),
        ),
        ("not_utf8", vec![0xff, 0xfe, 0x00]),
        ("not_json", b"{\"igz_format\": 1, \"scene\":".to_vec()),
        (
            "bad_document_json",
            zip_with(&[("document.json", b"[1, 2")]),
        ),
    ];
    for (name, bytes) in cases {
        let p = tmp.join(&format!("{name}.igz"));
        std::fs::write(&p, bytes).unwrap();
        match load(&p) {
            Err(IgzError::Corrupt(..) | IgzError::Json(_) | IgzError::Io(_)) => {}
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn test_a_format_4_document_is_refused() {
    let tmp = TmpDir::new();
    let path = tmp.join("next.igz");
    let doc = serde_json::json!({"igz_format": 4, "app_version": "0.6.0", "scene": {}});
    std::fs::write(
        &path,
        zip_with(&[("document.json", doc.to_string().as_bytes())]),
    )
    .unwrap();
    let err = load(&path).expect_err("refused");
    assert_eq!(
        err.to_string(),
        "IngeTrazo document format v4 is newer than this build reads (v3)."
    );
}

/// The format-3 fixture's entries, `geom/0.bin` (the loose mesh's block)
/// changed by `edit` (`None`: left out).
fn format3_with(edit: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
    let raw = std::fs::read(data("ingetrazo-0.5.8-format3.igz")).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut bytes).unwrap();
        let name = f.name().to_string();
        if name == "geom/0.bin" {
            match edit(&bytes) {
                Some(b) => entries.push((name, b)),
                None => continue,
            }
        } else {
            entries.push((name, bytes));
        }
    }
    let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), &b[..])).collect();
    zip_with(&refs)
}

#[test]
fn test_a_damaged_geometry_block_is_an_error_and_leaves_the_scene() {
    let tmp = TmpDir::new();
    let cases: [(&str, Vec<u8>, &str); 3] = [
        (
            "truncated",
            format3_with(|b| Some(b[..b.len() / 2].to_vec())),
            "damaged geometry block (",
        ),
        (
            "out_of_range",
            // the first edge's first end, an index into the points, set far past them
            format3_with(|b| {
                let mut b = b.to_vec();
                let at = 3733 * 12 + 4; // after the points, padded to 8 bytes
                b[at..at + 4].copy_from_slice(&1_000_000i32.to_le_bytes());
                Some(b)
            }),
            "damaged geometry block (edges)",
        ),
        (
            "missing",
            format3_with(|_| None),
            "the document lost its geometry block geom/0.bin",
        ),
    ];
    for (name, bytes, why) in cases {
        let path = tmp.join(&format!("{name}.igz"));
        std::fs::write(&path, bytes).unwrap();
        let mut s = Scene::new();
        let mut m = xtrude_core::Mesh::new();
        quad(&mut m, 0., 1., 0., 1., 0.);
        s.doc
            .meshes
            .insert(xtrude_model::ids::MeshId::LOOSE, std::sync::Arc::new(m));
        match igz::load_into(&mut s, &path, None) {
            Err(IgzError::Corrupt(file, msg)) => {
                assert_eq!(file, format!("{name}.igz"));
                assert!(msg.starts_with(why), "{name}: {msg}");
            }
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(
            s.mesh().faces.len(),
            1,
            "{name}: the scene is left as it was"
        );
    }
}

/// The format-3 fixture with its `document.json` changed by `edit`.
fn format3_with_document(edit: impl Fn(&mut serde_json::Value)) -> Vec<u8> {
    let raw = std::fs::read(data("ingetrazo-0.5.8-format3.igz")).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut bytes).unwrap();
        if f.name() == "document.json" {
            let mut doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            edit(&mut doc);
            bytes = doc.to_string().into_bytes();
        }
        entries.push((f.name().to_string(), bytes));
    }
    let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), &b[..])).collect();
    zip_with(&refs)
}

#[test]
fn test_one_geometry_block_may_be_used_twice_with_one_layout() {
    let tmp = TmpDir::new();
    let same = tmp.join("same.igz");
    std::fs::write(
        &same,
        format3_with_document(|d| d["scene"]["groups"][0]["geom"] = d["scene"]["geom"].clone()),
    )
    .unwrap();
    let mut s = Scene::new();
    igz::load_into(&mut s, &same, None).expect("opens");
    let plate = s.doc.mesh(s.doc.groups[0].mesh);
    assert_eq!(plate.faces.len(), s.mesh().faces.len());

    let other = tmp.join("other.igz");
    std::fs::write(
        &other,
        format3_with_document(|d| {
            d["scene"]["groups"][0]["geom"]["member"] = d["scene"]["geom"]["member"].clone()
        }),
    )
    .unwrap();
    match load(&other) {
        Err(IgzError::Corrupt(_, msg)) => assert_eq!(msg, "damaged geometry block (shared member)"),
        r => panic!("{r:?}"),
    }
}
