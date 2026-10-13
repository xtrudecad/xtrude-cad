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
