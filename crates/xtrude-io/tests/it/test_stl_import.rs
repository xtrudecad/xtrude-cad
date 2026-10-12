// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STL import: units, binary/ASCII detection, degenerate facets, big imports
//! grouped, flat merges.

use crate::common;
use common::*;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_io::stl::{self, unit_scale, SimplifyMode, StlError, MAX_FUSE_LOOPS};
use xtrude_model::commands::{ExplodeGroupCommand, SimplifyMeshCommand};
use xtrude_model::history::{FnCommand, History};
use xtrude_model::scene::Scene;

fn load(
    s: &mut Scene,
    path: &std::path::Path,
    scale: f64,
    mode: SimplifyMode,
) -> Result<(), StlError> {
    stl::load_stl(s, path, None, scale, mode)
}

#[test]
fn test_binary_stl_round_trip_rebuilds_closed_solid() {
    let mut original = Scene::new();
    let mut h = History::new();
    drawn_cube(&mut original, &mut h);
    let tmp = TmpDir::new();
    let path = tmp.join("cube.stl");
    stl::save_stl(&original, &path, "mm").unwrap();
    let mut imported = Scene::new();
    load(
        &mut imported,
        &path,
        unit_scale("mm").unwrap(),
        SimplifyMode::None,
    )
    .unwrap();
    let m = imported.mesh();
    assert_eq!(m.faces.len(), 6);
    assert_eq!(m.verts.len(), 8);
    assert!(is_closed(m));
    assert!((signed_volume(m) - 48.0).abs() < 1e-6 * 48.0);
}

#[test]
fn test_import_orients_solid_without_touching_open_scene_geometry() {
    let mut source = Scene::new();
    let mut h = History::new();
    drawn_cube(&mut source, &mut h);
    let tmp = TmpDir::new();
    let path = tmp.join("cube.stl");
    stl::save_stl(&source, &path, "mm").unwrap();
    let mut s = Scene::new();
    let existing = s
        .mesh_mut()
        .add_face(&[v3(10., 0., 0.), v3(11., 0., 0.), v3(10., 1., 0.)], &[]);
    load(&mut s, &path, unit_scale("mm").unwrap(), SimplifyMode::None).unwrap();
    assert!(s.mesh().has_face(existing));
    assert_eq!(s.mesh().faces.len(), 7);
    assert!((signed_volume(s.mesh()) - 48.0).abs() < 1e-6 * 48.0);
}

#[test]
fn test_binary_stl_solid_prefixed_header_is_detected_and_scaled() {
    let tmp = TmpDir::new();
    let path = tmp.join("solid_header.stl");
    write_binary_stl(
        &path,
        &[[[0., 0., 0.], [4000., 0., 0.], [0., 4000., 0.]]],
        b"solid binary file",
    );
    let mut s = Scene::new();
    load(&mut s, &path, 0.001, SimplifyMode::None).unwrap();
    assert!(approx6(face_bounds(&s), [0., 4., 0., 4., 0., 0.]));
}

#[test]
fn test_binary_stl_with_trailing_bytes_is_detected() {
    let tmp = TmpDir::new();
    let path = tmp.join("trailing_bytes.stl");
    write_binary_stl(
        &path,
        &[[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]],
        b"binary STL file",
    );
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"exporter metadata");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Scene::new();
    load(&mut s, &path, 1.0, SimplifyMode::None).unwrap();
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(approx6(face_bounds(&s), [0., 1., 0., 1., 0., 0.]));
}

#[test]
fn test_ascii_stl_is_whitespace_tolerant_and_scaled() {
    let tmp = TmpDir::new();
    let path = tmp.join("one_facet.stl");
    std::fs::write(
        &path,
        "SOLID part\n FACET NORMAL 0 0 1\n  OUTER   LOOP\n   VERTEX 0 0 0\n   vertex 4000 0 0\n   vertex 0 2000 0\n  ENDLOOP\n ENDFACET\nENDSOLID part\n",
    )
    .unwrap();
    let mut s = Scene::new();
    load(&mut s, &path, 0.001, SimplifyMode::None).unwrap();
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(approx6(face_bounds(&s), [0., 4., 0., 2., 0., 0.]));
}

#[test]
fn test_degenerate_facets_are_skipped() {
    let tmp = TmpDir::new();
    let path = tmp.join("degenerate.stl");
    write_binary_stl(
        &path,
        &[
            [[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]],
            [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        ],
        b"test STL",
    );
    let mut s = Scene::new();
    load(&mut s, &path, 1.0, SimplifyMode::None).unwrap();
    assert_eq!(s.mesh().faces.len(), 1);
}

#[test]
fn test_invalid_unit_scale_is_rejected() {
    let tmp = TmpDir::new();
    let path = tmp.join("triangle.stl");
    write_binary_stl(
        &path,
        &[[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]],
        b"test STL",
    );
    for scale in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        let err = load(&mut Scene::new(), &path, scale, SimplifyMode::None).unwrap_err();
        assert!(
            err.to_string().contains("positive finite"),
            "{scale}: {err}"
        );
    }
}

#[test]
fn test_malformed_binary_stl_is_rejected() {
    let tmp = TmpDir::new();
    let path = tmp.join("truncated.stl");
    let mut bytes = vec![0u8; 80];
    bytes.extend_from_slice(&1u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    let err = load(&mut Scene::new(), &path, 1.0, SimplifyMode::None).unwrap_err();
    assert!(err.to_string().contains("no usable triangles"), "{err}");
}

#[test]
fn test_large_stl_import_is_grouped_and_undoable() {
    let tmp = TmpDir::new();
    let path = tmp.join("large.stl");
    let tris: Vec<[[f64; 3]; 3]> = (0..=MAX_FUSE_LOOPS)
        .map(|i| {
            let x = i as f64 * 2.0;
            [[x, 0., 0.], [x + 1., 0., 0.], [x, 1., 0.]]
        })
        .collect();
    write_binary_stl(&path, &tris, b"test STL");
    let mut s = Scene::new();
    let mut h = History::new();
    let p = path.clone();
    let mut cmd = FnCommand::new("SnapshotImport", move |s: &mut Scene| {
        stl::load_stl(s, &p, None, 1.0, SimplifyMode::None)
            .map_err(|e| xtrude_model::history::CmdError::msg(e.to_string()))
    });
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    assert!(s.mesh().faces.is_empty());
    assert_eq!(s.doc.groups.len(), 1);
    assert_eq!(s.doc.mesh(s.doc.groups[0].mesh).faces.len(), tris.len());
    assert!(h.undo(&mut s));
    assert!(s.doc.groups.is_empty());
    assert!(h.redo(&mut s));
    assert_eq!(s.doc.groups.len(), 1);
}

fn grid(plane: &str) -> Vec<[[f64; 3]; 3]> {
    let point = |x: f64, y: f64| match plane {
        "XY" => [x, y, 0.],
        "XZ" => [x, 0., y],
        _ => [0., x, y],
    };
    let mut tris = Vec::new();
    for x in 0..21 {
        for y in 0..10 {
            let (x, y) = (x as f64, y as f64);
            let (a, b, c, d) = (
                point(x, y),
                point(x + 1., y),
                point(x + 1., y + 1.),
                point(x, y + 1.),
            );
            tris.push([a, b, c]);
            tris.push([a, c, d]);
        }
    }
    tris
}

#[test]
fn test_simplify_merges_large_coplanar_surface_on_principal_planes() {
    for plane in ["XY", "XZ", "YZ"] {
        let tmp = TmpDir::new();
        let path = tmp.join("flat_grid.stl");
        let tris = grid(plane);
        write_binary_stl(&path, &tris, b"test STL");
        let mut s = Scene::new();
        load(&mut s, &path, 1.0, SimplifyMode::Principal).unwrap();
        assert!(tris.len() > 400);
        assert_eq!(s.mesh().faces.len(), 1, "{plane}");
        assert_eq!(s.mesh().verts.len(), 4, "{plane}");
        let (bounds, n) = match plane {
            "XY" => ([0., 21., 0., 10., 0., 0.], v3(0., 0., 1.)),
            "XZ" => ([0., 21., 0., 0., 0., 10.], v3(0., -1., 0.)),
            _ => ([0., 0., 0., 21., 0., 10.], v3(1., 0., 0.)),
        };
        assert!(approx6(face_bounds(&s), bounds), "{plane}");
        let f = s.mesh().face_ids()[0];
        assert!(
            (s.mesh().face_normal(f).dot(n) - 1.0).abs() < 1e-6,
            "{plane}"
        );
    }
}

#[test]
fn test_advanced_simplification_merges_any_coplanar_surface() {
    let tmp = TmpDir::new();
    let path = tmp.join("tilted_grid.stl");
    let p = |x: f64, y: f64| [x, y, x + y];
    let (a, b, c, d) = (p(0., 0.), p(1., 0.), p(1., 1.), p(0., 1.));
    write_binary_stl(&path, &[[a, b, c], [a, c, d]], b"test STL");
    let mut principal = Scene::new();
    load(&mut principal, &path, 1.0, SimplifyMode::Principal).unwrap();
    assert_eq!(principal.mesh().faces.len(), 2);
    let mut advanced = Scene::new();
    load(&mut advanced, &path, 1.0, SimplifyMode::All).unwrap();
    assert_eq!(advanced.mesh().faces.len(), 1);
    assert_eq!(advanced.mesh().verts.len(), 4);
}

#[test]
fn test_simplify_keeps_curved_facets_separate() {
    let tmp = TmpDir::new();
    let path = tmp.join("curved_strip.stl");
    let segments = 405;
    let mut tris = Vec::new();
    for i in 0..segments {
        let a0 = std::f64::consts::PI * i as f64 / segments as f64;
        let a1 = std::f64::consts::PI * (i + 1) as f64 / segments as f64;
        let a = [10. * a0.cos(), 10. * a0.sin(), 0.];
        let b = [10. * a1.cos(), 10. * a1.sin(), 0.];
        let c = [10. * a1.cos(), 10. * a1.sin(), 1.];
        let d = [10. * a0.cos(), 10. * a0.sin(), 1.];
        tris.push([a, b, c]);
        tris.push([a, c, d]);
    }
    write_binary_stl(&path, &tris, b"test STL");
    let mut s = Scene::new();
    load(&mut s, &path, 1.0, SimplifyMode::All).unwrap();
    assert_eq!(s.doc.groups.len(), 1);
    let m = s.doc.mesh(s.doc.groups[0].mesh);
    let n = m.faces.len();
    // In f64 every planar quad merges, so n == segments.
    assert!(segments <= n && n < segments * 2, "{n}");
    // …and the point of the test, checked directly: no face spans two
    // facets of the curve.
    let step = std::f64::consts::PI / segments as f64;
    for f in m.face_ids() {
        let angles: Vec<f64> = m.face_positions(f).iter().map(|p| p.y.atan2(p.x)).collect();
        let (lo, hi) = angles
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(
            hi - lo <= step * 1.0001,
            "a face spans {} facets",
            (hi - lo) / step
        );
    }
}

#[test]
fn test_exploded_stl_group_can_be_simplified_as_an_undoable_edit() {
    let tmp = TmpDir::new();
    let path = tmp.join("large_flat_grid.stl");
    let tris = grid("XY");
    write_binary_stl(&path, &tris, b"test STL");
    let mut s = Scene::new();
    let mut h = History::new();
    load(&mut s, &path, 1.0, SimplifyMode::None).unwrap();
    assert_eq!(s.doc.groups.len(), 1);
    let gid = s.doc.groups[0].id;
    h.execute(&mut s, &mut ExplodeGroupCommand { group: gid });
    assert!(s.doc.groups.is_empty());
    assert_eq!(s.mesh().faces.len(), tris.len());
    h.execute(
        &mut s,
        &mut SimplifyMeshCommand {
            max_angle_degrees: 0.0,
            faces_removed: 0,
        },
    );
    assert!(h.last_error.is_none());
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), tris.len());
    assert!(h.redo(&mut s));
    assert_eq!(s.mesh().faces.len(), 1);
}
