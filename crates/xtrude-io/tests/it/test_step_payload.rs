// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The STEP import without OpenCascade: the helper's stream, the turn of
//! its meshes into the import payload, the payload applied to a scene,
//! the tessellation setting, and the runner that starts the helper (with
//! stand-in scripts for the helper).

use xtrude_core::math::DMat4;
use xtrude_core::DVec3;
use xtrude_io::step::protocol::{decode, encode, RawFace, RawInstance, RawPart, RawStep};
use xtrude_io::step::{
    apply_payload, is_step_path, StepError, StepPayload, StepWarning, Tessellation,
};
use xtrude_model::group::world_mesh;
use xtrude_model::scene::Scene;
use xtrude_model::settings::{self, keys};
use xtrude_model::solids::solid_volume;

const RED: [f64; 3] = [0.8, 0.1, 0.1];
const GREEN: [f64; 3] = [0.1, 0.7, 0.2];

/// A planar face from its corners, counter-clockwise seen from outside.
fn quad(corners: [[f64; 3]; 4]) -> RawFace {
    RawFace {
        positions: corners.iter().map(|&c| DVec3::from(c)).collect(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        planar: true,
        color: None,
    }
}

/// The six faces of a cube of side `s` at the origin, outward.
fn cube_faces(s: f64) -> Vec<RawFace> {
    vec![
        quad([[0., 0., 0.], [0., s, 0.], [s, s, 0.], [s, 0., 0.]]),
        quad([[0., 0., s], [s, 0., s], [s, s, s], [0., s, s]]),
        quad([[0., 0., 0.], [s, 0., 0.], [s, 0., s], [0., 0., s]]),
        quad([[0., s, 0.], [0., s, s], [s, s, s], [s, s, 0.]]),
        quad([[0., 0., 0.], [0., 0., s], [0., s, s], [0., s, 0.]]),
        quad([[s, 0., 0.], [s, s, 0.], [s, s, s], [s, 0., s]]),
    ]
}

fn instance(part: usize, name: &str, x: f64) -> RawInstance {
    RawInstance {
        part,
        name: name.into(),
        xform: DMat4::from_translation(DVec3::new(x, 0.0, 0.0)),
    }
}

/// A red 10 mm cube with one green face, placed twice in an assembly.
fn assembly() -> RawStep {
    let mut faces = cube_faces(0.01);
    faces[1].color = Some(GREEN);
    let cube = RawPart {
        name: "Bracket".into(),
        color: Some(RED),
        faces,
        closed: true,
        ..Default::default()
    };
    let asm = RawPart {
        name: "Assembly".into(),
        children: vec![instance(0, "Left", 0.0), instance(0, "Right", 0.05)],
        ..Default::default()
    };
    RawStep {
        parts: vec![cube, asm],
        roots: vec![instance(1, "Assembly", 0.0)],
    }
}

fn one_part(part: RawPart) -> RawStep {
    RawStep {
        parts: vec![part],
        roots: vec![instance(0, "", 0.0)],
    }
}

// ---- The stream --------------------------------------------------------------

#[test]
fn test_the_stream_round_trips() {
    let mut raw = assembly();
    raw.parts[0].failed_faces = 2;
    raw.parts[1].children[1].xform =
        DMat4::from_rotation_z(0.5) * DMat4::from_translation(DVec3::new(0.05, -0.02, 0.003));
    assert_eq!(decode(&encode(&raw)).unwrap(), raw);
}

#[test]
fn test_a_broken_stream_is_an_error() {
    let good = encode(&assembly());
    assert!(decode(b"").is_err());
    assert!(decode(b"NOTASTREAM\x01\x00").is_err());
    let mut version = good.clone();
    version[10] = 9;
    assert!(decode(&version).is_err(), "another version");
    for cut in [11, 20, good.len() / 2, good.len() - 1] {
        assert!(decode(&good[..cut]).is_err(), "truncated at {cut}");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(decode(&trailing).is_err(), "trailing bytes");
    // A count far beyond the stream: refused before allocating.
    let mut huge = good[..12].to_vec();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&huge).is_err());
}

#[test]
fn test_out_of_range_indices_are_refused() {
    let mut bad_tri = assembly();
    bad_tri.parts[0].faces[0].triangles[0][2] = 4;
    assert!(decode(&encode(&bad_tri)).is_err());
    let mut bad_part = assembly();
    bad_part.roots[0].part = 2;
    assert!(decode(&encode(&bad_part)).is_err());
}

// ---- Into the payload --------------------------------------------------------

#[test]
fn test_a_planar_face_becomes_one_polygon_with_its_hole() {
    // A 4 cm square with a 2 cm square hole, as eight triangles.
    let c = 0.01;
    let outer = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]];
    let inner = [[1., 1.], [3., 1.], [3., 3.], [1., 3.]];
    let positions: Vec<DVec3> = outer
        .iter()
        .chain(&inner)
        .map(|p| DVec3::new(p[0] * c, p[1] * c, 0.0))
        .collect();
    let mut triangles = Vec::new();
    for k in 0..4u32 {
        let n = (k + 1) % 4;
        triangles.push([k, n, 4 + n]);
        triangles.push([k, 4 + n, 4 + k]);
    }
    let face = RawFace {
        positions,
        triangles,
        planar: true,
        color: None,
    };
    let payload = StepPayload::from_raw(&one_part(RawPart {
        faces: vec![face.clone()],
        ..Default::default()
    }))
    .unwrap();
    let faces = &payload.model.protos[0].faces;
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].outer.len(), 4);
    assert_eq!(faces[0].holes.len(), 1);
    assert_eq!(faces[0].holes[0].len(), 4);
    assert_eq!(payload.model.protos[0].soft_edges, Some(vec![]));

    // Not planar: the triangles stay, joined by soft edges.
    let curved = RawFace {
        planar: false,
        ..face
    };
    let payload = StepPayload::from_raw(&one_part(RawPart {
        faces: vec![curved],
        ..Default::default()
    }))
    .unwrap();
    let proto = &payload.model.protos[0];
    assert_eq!(proto.faces.len(), 8);
    assert_eq!(proto.soft_edges.as_ref().map(Vec::len), Some(8));
}

#[test]
fn test_faces_below_the_weld_tolerance_are_left_out_with_a_warning() {
    let mut faces = cube_faces(0.01);
    // 10 µm across: every corner welds to one vertex.
    let tiny = 1e-5;
    faces.push(RawFace {
        positions: vec![
            DVec3::new(0.02, 0.0, 0.0),
            DVec3::new(0.02 + tiny, 0.0, 0.0),
            DVec3::new(0.02, tiny, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        planar: true,
        color: None,
    });
    let payload = StepPayload::from_raw(&one_part(RawPart {
        faces,
        closed: true,
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(payload.model.protos[0].faces.len(), 6);
    assert_eq!(
        payload.warnings,
        vec![StepWarning::SmallFeatures { count: 1 }]
    );
}

#[test]
fn test_repeated_and_folded_triangles_are_dropped() {
    // A curved face: a strip of two triangles, one of them repeated, plus
    // a sliver folded back over the first.
    let face = RawFace {
        positions: vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.01, 0.0, 0.0),
            DVec3::new(0.01, 0.01, 0.001),
            DVec3::new(0.0, 0.01, 0.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [2, 3, 0], [1, 0, 2], [0, 1, 2]],
        planar: false,
        color: None,
    };
    let payload = StepPayload::from_raw(&one_part(RawPart {
        faces: vec![face],
        ..Default::default()
    }))
    .unwrap();
    // [0, 1, 2] and its fold [1, 0, 2] cancel; the later [0, 1, 2] comes
    // back once; [2, 3, 0] repeats [0, 2, 3].
    let proto = &payload.model.protos[0];
    assert_eq!(proto.faces.len(), 2);
    assert_eq!(proto.soft_edges.as_ref().map(Vec::len), Some(1));
}

#[test]
fn test_open_shells_and_unmeshed_faces_are_warned_about() {
    let payload = StepPayload::from_raw(&one_part(RawPart {
        name: "Lid".into(),
        faces: cube_faces(0.01)[..5].to_vec(),
        closed: false,
        failed_faces: 1,
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(
        payload.warnings,
        vec![
            StepWarning::OpenShell { part: "Lid".into() },
            StepWarning::FacesNotMeshed {
                part: "Lid".into(),
                count: 1
            },
        ]
    );
    assert!(payload.warnings[0].to_string().contains("Lid"));

    // A part without a name of its own goes by the name it is placed under.
    let mut raw = one_part(RawPart {
        faces: cube_faces(0.01)[..5].to_vec(),
        ..Default::default()
    });
    raw.roots[0].name = "Cover".into();
    let payload = StepPayload::from_raw(&raw).unwrap();
    assert_eq!(
        payload.warnings,
        vec![StepWarning::OpenShell {
            part: "Cover".into()
        }]
    );
}

#[test]
fn test_a_closed_part_the_weld_opens_is_warned_about() {
    // The reader found the shell closed, but one side is gone once welded
    // (here: left out), as when a feature is thinner than the tolerance.
    let payload = StepPayload::from_raw(&one_part(RawPart {
        name: "Clip".into(),
        faces: cube_faces(0.01)[..5].to_vec(),
        closed: true,
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(
        payload.warnings,
        vec![StepWarning::ThinFeatures {
            part: "Clip".into()
        }]
    );
    let whole = StepPayload::from_raw(&one_part(RawPart {
        faces: cube_faces(0.01),
        closed: true,
        ..Default::default()
    }))
    .unwrap();
    assert!(whole.warnings.is_empty());
}

#[test]
fn test_an_unnamed_part_is_named_after_the_assembly_holding_it() {
    // An open sheet without a name, beside a closed body, in a product.
    let raw = RawStep {
        parts: vec![
            RawPart {
                faces: cube_faces(0.01),
                closed: true,
                ..Default::default()
            },
            RawPart {
                faces: cube_faces(0.01)[..1].to_vec(),
                ..Default::default()
            },
            RawPart {
                name: "Housing".into(),
                children: vec![instance(0, "", 0.0), instance(1, "", 0.0)],
                ..Default::default()
            },
        ],
        roots: vec![instance(2, "", 0.0)],
    };
    let payload = StepPayload::from_raw(&raw).unwrap();
    assert_eq!(
        payload.warnings,
        vec![StepWarning::OpenShell {
            part: "Housing › unnamed part".into()
        }]
    );
}

#[test]
fn test_a_file_without_faces_has_no_geometry() {
    let empty = RawStep {
        parts: vec![RawPart {
            name: "Empty assembly".into(),
            children: vec![instance(0, "self", 0.0)],
            ..Default::default()
        }],
        roots: vec![instance(0, "", 0.0)],
    };
    assert_eq!(StepPayload::from_raw(&empty), Err(StepError::NoGeometry));
    assert_eq!(
        StepPayload::from_raw(&RawStep::default()),
        Err(StepError::NoGeometry)
    );
}

// ---- Applied ------------------------------------------------------------------

#[test]
fn test_an_assembly_imports_as_named_nested_groups_sharing_one_mesh() {
    let mut scene = Scene::new();
    let payload = StepPayload::from_raw(&assembly()).unwrap();
    assert!(payload.warnings.is_empty());
    apply_payload(&mut scene, &payload).unwrap();

    assert_eq!(scene.doc.groups.len(), 1);
    let top = &scene.doc.groups[0];
    assert_eq!(top.name, "Assembly");
    let names: Vec<&str> = top.children.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Left", "Right"]);
    let (left, right) = (&top.children[0], &top.children[1]);
    assert!(left.component && right.component);
    assert_eq!(left.mesh, right.mesh, "the instances share the part's mesh");
    let x = |g: &xtrude_model::group::Group| g.xform.unwrap().w_axis.x;
    assert!((x(left) - 0.0).abs() < 1e-12);
    assert!((x(right) - 0.05).abs() < 1e-12);

    // Each instance is a closed 10 mm cube.
    for g in [left, right] {
        let volume = solid_volume(&scene.doc.meshes, g).expect("a solid");
        assert!((volume - 1e-6).abs() < 1e-12, "volume {volume}");
    }
    let world = world_mesh(&scene.doc.meshes, right);
    let xs: Vec<f64> = world
        .face_ids()
        .iter()
        .flat_map(|&f| world.face_positions(f))
        .map(|p| p.x)
        .collect();
    let (lo, hi) = xs
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    assert!((lo - 0.05).abs() < 1e-9 && (hi - 0.06).abs() < 1e-9);

    // The part's colour, and the face's own over it.
    let mesh = scene.doc.mesh(left.mesh);
    let mut colors: Vec<[f64; 3]> = mesh
        .face_ids()
        .iter()
        .filter_map(|&f| mesh.face(f).attrs.paint.color)
        .collect();
    colors.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(colors, [GREEN, RED, RED, RED, RED, RED]);
}

#[test]
fn test_a_single_part_is_one_group_named_after_it() {
    let mut scene = Scene::new();
    let raw = RawStep {
        parts: vec![RawPart {
            name: "Cube".into(),
            faces: cube_faces(0.01),
            closed: true,
            ..Default::default()
        }],
        roots: vec![instance(0, "Cube", 0.0)],
    };
    apply_payload(&mut scene, &StepPayload::from_raw(&raw).unwrap()).unwrap();
    assert_eq!(scene.doc.groups.len(), 1);
    let g = &scene.doc.groups[0];
    assert_eq!(g.name, "Cube");
    assert!(!g.component, "placed once: not a component");
    assert_eq!(
        scene.doc.mesh(g.mesh).faces.len(),
        6,
        "one face per planar face"
    );
}

#[test]
fn test_single_child_assemblies_fold_into_one_group() {
    // product ▸ compound ▸ solid, as OpenCascade reads a one-part file.
    let raw = RawStep {
        parts: vec![
            RawPart {
                name: "SOLID".into(),
                faces: cube_faces(0.01),
                closed: true,
                ..Default::default()
            },
            RawPart {
                name: "COMPOUND".into(),
                children: vec![instance(0, "SOLID", 0.02)],
                ..Default::default()
            },
            RawPart {
                name: "Bracket".into(),
                children: vec![instance(1, "COMPOUND", 0.01)],
                ..Default::default()
            },
        ],
        roots: vec![instance(2, "Bracket", 0.0)],
    };
    let mut scene = Scene::new();
    apply_payload(&mut scene, &StepPayload::from_raw(&raw).unwrap()).unwrap();
    assert_eq!(scene.doc.groups.len(), 1);
    let g = &scene.doc.groups[0];
    assert_eq!(g.name, "Bracket");
    assert!(g.children.is_empty());
    assert!(
        (g.xform.unwrap().w_axis.x - 0.03).abs() < 1e-12,
        "the placements compose"
    );
    assert!(solid_volume(&scene.doc.meshes, g).is_some());
}

// ---- The setting --------------------------------------------------------------

#[test]
fn test_the_tessellation_setting() {
    settings::use_memory_store();
    let stored = settings::read(&keys::STEP_TESSELLATION);
    assert_eq!(Tessellation::from_json(&stored.0), Tessellation::default());
    let d = Tessellation::default();
    assert!((d.chord - 1e-5).abs() < 1e-15);
    assert_eq!(d.angle_deg, 10.0);

    let t = Tessellation {
        chord: 0.05e-3,
        angle_deg: 15.0,
    };
    let back = Tessellation::from_json(&t.to_json());
    assert!((back.chord - t.chord).abs() < 1e-15);
    assert_eq!(back.angle_deg, 15.0);

    // Out of range or not a number: that value's default.
    let odd = serde_json::json!({ "chord_mm": 50.0, "angle_deg": "fine" });
    assert_eq!(Tessellation::from_json(&odd), d);
    let half = serde_json::json!({ "chord_mm": 0.1 });
    assert_eq!(Tessellation::from_json(&half).angle_deg, 10.0);
}

#[test]
fn test_step_extensions() {
    use std::path::Path;
    for p in ["board.step", "board.STP", "a/b.Step"] {
        assert!(is_step_path(Path::new(p)), "{p}");
    }
    for p in ["board.stl", "board", "step"] {
        assert!(!is_step_path(Path::new(p)), "{p}");
    }
}

// ---- The runner ---------------------------------------------------------------

#[cfg(unix)]
mod runner {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use xtrude_io::step::protocol::encode;
    use xtrude_io::step::{parse_step_with, HelperBackend, StepError, StepPayload};

    use super::{assembly, Tessellation};
    use crate::common::TmpDir;

    /// A stand-in helper: a shell script.
    fn script(dir: &TmpDir, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn parse(program: &Path, cancel: Option<&AtomicBool>) -> Result<StepPayload, StepError> {
        let backend = HelperBackend::new(program);
        let file = Path::new("part.step");
        loop {
            match parse_step_with(&backend, file, &Tessellation::default(), None, cancel) {
                // A script just written can be busy for an instant while
                // another test thread starts a process.
                Err(StepError::Io(msg)) if msg.contains("busy") => continue,
                other => return other,
            }
        }
    }

    #[test]
    fn test_a_missing_or_stub_helper_is_not_installed() {
        let dir = TmpDir::new();
        assert_eq!(
            parse(&dir.join("no-such-helper"), None),
            Err(StepError::NotInstalled)
        );
        let stub = script(&dir, "stub", "echo 'built without OpenCascade' >&2; exit 3");
        assert_eq!(parse(&stub, None), Err(StepError::NotInstalled));
    }

    #[test]
    fn test_helper_failures_become_errors() {
        let dir = TmpDir::new();
        let fails = script(&dir, "fails", "echo 'not a readable STEP file' >&2; exit 1");
        assert_eq!(
            parse(&fails, None),
            Err(StepError::Read("not a readable STEP file".into()))
        );
        let crashes = script(&dir, "crashes", "kill -SEGV $$");
        assert_eq!(parse(&crashes, None), Err(StepError::Crashed));
        let garbage = script(&dir, "garbage", "echo hello");
        assert!(matches!(
            parse(&garbage, None),
            Err(StepError::BadOutput(_))
        ));
    }

    #[test]
    fn test_the_helper_gets_the_tolerances_and_reports_progress() {
        let dir = TmpDir::new();
        let stream = dir.join("stream.bin");
        std::fs::write(&stream, encode(&assembly())).unwrap();
        let args = dir.join("args.txt");
        let helper = script(
            &dir,
            "helper",
            &format!(
                "echo \"$@\" > '{}'\necho '@progress 0.25 Reading the file…' >&2\ncat '{}'",
                args.display(),
                stream.display()
            ),
        );
        let mut seen = Vec::new();
        let mut progress = |f: f64, text: &str| seen.push((f, text.to_string()));
        let backend = HelperBackend::new(&helper);
        let payload = loop {
            match parse_step_with(
                &backend,
                Path::new("part.step"),
                &Tessellation::default(),
                Some(&mut progress),
                None,
            ) {
                Err(StepError::Io(msg)) if msg.contains("busy") => continue,
                other => break other.unwrap(),
            }
        };
        assert_eq!(payload, StepPayload::from_raw(&assembly()).unwrap());
        assert_eq!(seen, [(0.25, "Reading the file…".to_string())]);
        let args = std::fs::read_to_string(&args).unwrap();
        assert_eq!(args.trim(), "part.step --chord-mm 0.01 --angle-deg 10");
    }

    #[test]
    fn test_cancel_kills_the_helper() {
        let dir = TmpDir::new();
        let slow = script(&dir, "slow", "exec sleep 30");
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let result = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(200));
                cancel.store(true, Ordering::Relaxed);
            });
            parse(&slow, Some(&cancel))
        });
        assert_eq!(result, Err(StepError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
