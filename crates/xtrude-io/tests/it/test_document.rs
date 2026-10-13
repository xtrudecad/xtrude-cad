// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Xtrude CAD document (`.xtrude`): its envelope, the formats it refuses,
//! damaged files, and `xtrude-convert`, which reads `.igz` but never
//! writes one.

use crate::common;
use common::*;
use std::path::Path;
use std::process::{Command, Output};
use xtrude_io::document::{self, DocError, FORMAT};
use xtrude_io::igz;
use xtrude_model::scene::Scene;

fn square_scene() -> Scene {
    let mut s = Scene::new();
    quad(s.mesh_mut(), 0., 1., 0., 1., 0.);
    s
}

fn load(path: &Path) -> Result<Scene, DocError> {
    let mut s = Scene::new();
    document::load_into(&mut s, path, None).map(|_| s)
}

#[test]
fn test_the_extension_and_mime_type() {
    assert_eq!(document::EXTENSION, "xtrude");
    assert_eq!(document::MIME, "application/x-xtrudecad");
    assert!(document::is_document_path(Path::new("part.XTRUDE")));
    assert!(!document::is_document_path(Path::new("part.igz")));
}

#[test]
fn test_a_newer_format_is_refused() {
    let tmp = TmpDir::new();
    let path = tmp.join("future.xtrude");
    document::save_scene(&square_scene(), &path).unwrap();
    let mut doc = document_json(&path);
    doc["xtrude_format"] = (FORMAT + 1).into();
    write_document_json(&path, &doc);
    let err = load(&path).err().expect("refused");
    assert!(
        matches!(err, DocError::TooNew(n) if n == FORMAT + 1),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "Document format v{} is newer than this build (v{FORMAT}).",
            FORMAT + 1
        )
    );
}

#[test]
fn test_a_damaged_file_is_an_error_never_a_panic() {
    let tmp = TmpDir::new();
    let path = tmp.join("good.xtrude");
    document::save_scene(&square_scene(), &path).unwrap();
    let good = std::fs::read(&path).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("truncated", good[..good.len() / 2].to_vec()),
        ("garbage", b"\x1f\x8b\x08\x00 not really gzip".to_vec()),
        ("binary", vec![0xff, 0xfe, 0x00, 0x01]),
        ("text", b"hello".to_vec()),
        ("array", b"[1, 2, 3]".to_vec()),
    ];
    for (name, bytes) in cases {
        let p = tmp.join(&format!("{name}.xtrude"));
        std::fs::write(&p, bytes).unwrap();
        match load(&p) {
            Err(DocError::Corrupt(..) | DocError::NotADocument(_)) => {}
            other => panic!("{name}: {:?}", other.map(|_| "opened")),
        }
    }
    let mut no_scene = document_json(&path);
    no_scene.as_object_mut().unwrap().remove("scene");
    let p = tmp.join("no_scene.xtrude");
    write_document_json(&p, &no_scene);
    assert!(matches!(load(&p), Err(DocError::Corrupt(..))));
    assert!(matches!(
        load(&tmp.join("missing.xtrude")),
        Err(DocError::Io(_))
    ));
}

#[test]
fn test_an_igz_is_not_a_document_and_a_document_is_not_an_igz() {
    let tmp = TmpDir::new();
    let igz_path = tmp.join("old.igz");
    write_igz(&igz_path, &square_scene());
    assert!(matches!(load(&igz_path), Err(DocError::NotADocument(_))));
    let xtrude = tmp.join("new.xtrude");
    document::save_scene(&square_scene(), &xtrude).unwrap();
    assert!(igz::load_into(&mut Scene::new(), &xtrude, None).is_err());
}

#[test]
fn test_an_unpacked_document_still_opens() {
    let tmp = TmpDir::new();
    let path = tmp.join("square.xtrude");
    document::save_scene(&square_scene(), &path).unwrap();
    let text = document_text(&path);
    std::fs::write(&path, text).unwrap();
    assert_eq!(load(&path).unwrap().mesh().faces.len(), 1);
}

#[test]
fn test_saving_the_same_document_writes_the_same_bytes() {
    let tmp = TmpDir::new();
    let (a, b) = (tmp.join("a.xtrude"), tmp.join("b.xtrude"));
    document::save_scene(&square_scene(), &a).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    document::save_scene(&square_scene(), &b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn test_an_igz_opened_and_saved_becomes_a_document() {
    let mut s = Scene::new();
    igz::load_into(&mut s, &data("upstream_removed_features.igz"), None).unwrap();
    let tmp = TmpDir::new();
    let out = tmp.join("again.xtrude");
    document::save_scene(&s, &out).unwrap();
    let doc = document_json(&out);
    assert_eq!(doc["xtrude_format"], FORMAT);
    assert_eq!(doc["app_version"], xtrude_model::VERSION);
    assert!(doc.get("igz_format").is_none());
}

// ---- xtrude-convert ---------------------------------------------------------------------

fn convert(input: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtrude-convert"))
        .arg(input)
        .arg(output)
        .output()
        .expect("xtrude-convert runs")
}

#[test]
fn test_convert_reads_an_igz_and_writes_a_document() {
    let tmp = TmpDir::new();
    let out = tmp.join("upstream.xtrude");
    let run = convert(&data("upstream_removed_features.igz"), &out);
    assert!(run.status.success(), "{run:?}");
    let s = load(&out).unwrap();
    assert_eq!(s.mesh().faces.len(), 6);
    let stl = tmp.join("upstream.stl");
    assert!(convert(&out, &stl).status.success());
    assert!(stl.is_file());
}

#[test]
fn test_convert_never_writes_an_igz() {
    let tmp = TmpDir::new();
    let doc = tmp.join("square.xtrude");
    document::save_scene(&square_scene(), &doc).unwrap();
    let out = tmp.join("square.igz");
    let run = convert(&doc, &out);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("cannot write .igz"));
    assert!(!out.exists());
}
