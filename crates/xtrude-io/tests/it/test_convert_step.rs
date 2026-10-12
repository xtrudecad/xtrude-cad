// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `xtrude-convert part.step part.stl`: a STEP file converts to the other
//! formats through the STEP helper, which the converter is pointed at with
//! `XTRUDE_STEP_HELPER` (set on its process only); without a helper it
//! fails and says why.

use crate::common;

use std::path::Path;
use std::process::{Command, Output};

use common::step::{fixture, helper};
use common::TmpDir;
use xtrude_io::step::helper::HELPER_ENV;

fn convert(input: &Path, output: &Path, helper: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtrude-convert"))
        .arg(input)
        .arg(output)
        .env(HELPER_ENV, helper)
        .output()
        .expect("xtrude-convert runs")
}

/// The triangles of a binary STL, in millimetres.
fn stl_triangles(path: &Path) -> Vec<[[f32; 3]; 3]> {
    let bytes = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    assert_eq!(bytes.len(), 84 + 50 * n);
    let f =
        |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    (0..n)
        .map(|i| {
            let base = 84 + 50 * i + 12;
            let mut t = [[0f32; 3]; 3];
            for (k, corner) in t.iter_mut().enumerate() {
                for (c, v) in corner.iter_mut().enumerate() {
                    *v = f(base + 12 * k + 4 * c);
                }
            }
            t
        })
        .collect()
}

fn size(tris: &[[[f32; 3]; 3]]) -> [f32; 3] {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

#[test]
fn test_a_step_file_converts_to_stl() {
    let Some(h) = helper() else { return };
    let dir = TmpDir::new();
    let out = dir.join("assembly.stl");
    let run = convert(&fixture("assembly_colours.step"), &out, &h.program);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let tris = stl_triangles(&out);
    let s = size(&tris);
    for (got, want) in s.iter().zip([80.0, 30.0, 6.0]) {
        assert!((got - want).abs() < 1e-3, "size {s:?} mm");
    }
}

#[test]
fn test_without_a_helper_the_conversion_fails_and_says_why() {
    let dir = TmpDir::new();
    let out = dir.join("cube.stl");
    let run = convert(&fixture("cube.step"), &out, &dir.join("no-helper-here"));
    assert!(!run.status.success());
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("STEP support is not installed"), "{err}");
    assert!(!out.exists());
}

/// Without OpenCascade: a stand-in helper writes a 10 mm cube with one
/// side missing, which converts, the warning printed on stderr.
#[cfg(unix)]
#[test]
fn test_a_stand_in_helper_converts_and_its_warnings_are_printed() {
    use std::os::unix::fs::PermissionsExt;
    use xtrude_core::math::DMat4;
    use xtrude_core::DVec3;
    use xtrude_io::step::protocol::{encode, RawFace, RawInstance, RawPart, RawStep};

    let s = 0.01;
    let quads = [
        [[0., 0., 0.], [0., s, 0.], [s, s, 0.], [s, 0., 0.]],
        [[0., 0., 0.], [s, 0., 0.], [s, 0., s], [0., 0., s]],
        [[0., s, 0.], [0., s, s], [s, s, s], [s, s, 0.]],
        [[0., 0., 0.], [0., 0., s], [0., s, s], [0., s, 0.]],
        [[s, 0., 0.], [s, s, 0.], [s, s, s], [s, 0., s]],
    ];
    let raw = RawStep {
        parts: vec![RawPart {
            name: "Tray".into(),
            faces: quads
                .iter()
                .map(|q| RawFace {
                    positions: q.iter().map(|&c| DVec3::from(c)).collect(),
                    triangles: vec![[0, 1, 2], [0, 2, 3]],
                    planar: true,
                    color: None,
                })
                .collect(),
            ..Default::default()
        }],
        roots: vec![RawInstance {
            part: 0,
            name: "Tray".into(),
            xform: DMat4::IDENTITY,
        }],
    };
    let dir = TmpDir::new();
    let stream = dir.join("stream.bin");
    std::fs::write(&stream, encode(&raw)).unwrap();
    let script = dir.join("xtrude-step");
    std::fs::write(&script, format!("#!/bin/sh\ncat '{}'\n", stream.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = dir.join("tray.stl");
    // A script just written can be busy an instant: try again.
    let run = (0..20)
        .map(|_| convert(Path::new("tray.step"), &out, &script))
        .find(|run| !String::from_utf8_lossy(&run.stderr).contains("busy"))
        .expect("the stand-in helper stayed busy");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{err}");
    assert!(err.contains("“Tray” is not closed"), "{err}");
    let tris = stl_triangles(&out);
    assert_eq!(tris.len(), 10);
    let s = size(&tris);
    for got in s {
        assert!((got - 10.0).abs() < 1e-4, "size {s:?} mm");
    }
}
