// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STL and OBJ export.

use crate::common;
use common::*;
use std::path::Path;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::{DVec3, Mesh};
use xtrude_io::stl::MAX_FUSE_LOOPS;
use xtrude_io::{obj, stl};
use xtrude_model::commands::SetFaceColorCommand;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::history::{FnCommand, History};
use xtrude_model::scene::Scene;

fn expected_tri_count(s: &Scene) -> usize {
    s.render_faces()
        .iter()
        .map(|&(m, f)| s.doc.mesh(m).face_triangulate(f, None).len())
        .sum()
}

type StlTri = ([f32; 3], [f32; 3], [f32; 3], [f32; 3]);

fn read_stl(path: &Path) -> Vec<StlTri> {
    let b = std::fs::read(path).unwrap();
    let count = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    (0..count)
        .map(|i| {
            let r = &b[84 + 50 * i..84 + 50 * (i + 1)];
            let f =
                |k: usize| f32::from_le_bytes([r[4 * k], r[4 * k + 1], r[4 * k + 2], r[4 * k + 3]]);
            let v = |k: usize| [f(k), f(k + 1), f(k + 2)];
            (v(0), v(3), v(6), v(9))
        })
        .collect()
}

fn cube() -> (Scene, History) {
    let mut s = Scene::new();
    let mut h = History::new();
    drawn_cube(&mut s, &mut h);
    (s, h)
}

#[test]
fn test_stl_triangle_count_matches() {
    let (s, _h) = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.stl");
    stl::save_stl(&s, &path, "mm").unwrap();
    let tris = read_stl(&path);
    assert_eq!(tris.len(), expected_tri_count(&s));
    assert_eq!(tris.len(), 12);
}

#[test]
fn test_stl_normals_point_outward() {
    let (mut s, _h) = cube();
    orient_outward(s.mesh_mut(), DEFAULT_SEED, None);
    let centroid = DVec3::new(2., 2., 1.5) * 1000.0;
    let tmp = TmpDir::new();
    let path = tmp.join("cube.stl");
    stl::save_stl(&s, &path, "mm").unwrap();
    for (n, a, b, c) in read_stl(&path) {
        let nv = DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64);
        let tc = DVec3::new(
            (a[0] + b[0] + c[0]) as f64 / 3.0,
            (a[1] + b[1] + c[1]) as f64 / 3.0,
            (a[2] + b[2] + c[2]) as f64 / 3.0,
        );
        assert!(nv.dot(tc - centroid) > 0.0);
    }
}

#[test]
fn test_stl_is_written_in_millimetres() {
    let (s, _h) = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.stl");
    stl::save_stl(&s, &path, "mm").unwrap();
    let max = |path: &Path, k: usize| {
        read_stl(path)
            .iter()
            .flat_map(|t| [t.1[k], t.2[k], t.3[k]])
            .fold(f32::MIN, f32::max) as f64
    };
    assert!((max(&path, 0) - 4000.0).abs() < 1e-3);
    assert!((max(&path, 1) - 4000.0).abs() < 1e-3);
    assert!((max(&path, 2) - 3000.0).abs() < 1e-3);
    stl::save_stl(&s, &path, "m").unwrap();
    assert!((max(&path, 2) - 3.0).abs() < 1e-6);
}

#[test]
fn test_stl_includes_groups() {
    let (mut s, _h) = cube();
    let mut m = Mesh::new();
    let p = [
        v3(10., 0., 0.),
        v3(12., 0., 0.),
        v3(12., 2., 0.),
        v3(10., 2., 0.),
    ];
    let q = [
        v3(10., 0., 2.),
        v3(12., 0., 2.),
        v3(12., 2., 2.),
        v3(10., 2., 2.),
    ];
    m.add_face(&p, &[]);
    m.add_face(&q, &[]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        m.add_face(&[p[i], p[j], q[j], q[i]], &[]);
    }
    let g = Group::new(add_mesh(&mut s.doc.meshes, m), None);
    s.doc.groups.push(g);
    let tmp = TmpDir::new();
    let path = tmp.join("with_group.stl");
    stl::save_stl(&s, &path, "mm").unwrap();
    assert_eq!(read_stl(&path).len(), expected_tri_count(&s));
}

/// An OBJ face: its material and vertex indices.
type ObjFace = (Option<String>, Vec<usize>);

fn read_obj(path: &Path) -> (Vec<[f64; 3]>, Vec<ObjFace>) {
    let mut verts = Vec::new();
    let mut faces = Vec::new();
    let mut cur = None;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(rest) = line.strip_prefix("v ") {
            let v: Vec<f64> = rest
                .split_whitespace()
                .map(|x| x.parse().unwrap())
                .collect();
            verts.push([v[0], v[1], v[2]]);
        } else if let Some(rest) = line.strip_prefix("usemtl ") {
            cur = Some(rest.split_whitespace().next().unwrap().to_string());
        } else if let Some(rest) = line.strip_prefix("f ") {
            faces.push((
                cur.clone(),
                rest.split_whitespace()
                    .map(|t| t.split('/').next().unwrap().parse().unwrap())
                    .collect(),
            ));
        }
    }
    (verts, faces)
}

#[test]
fn test_obj_vertices_deduped_and_indices_valid() {
    let (s, _h) = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.obj");
    obj::save_obj(&s, &path).unwrap();
    let (verts, faces) = read_obj(&path);
    assert_eq!(verts.len(), 8);
    assert_eq!(faces.len(), 12);
    for (_m, idxs) in faces {
        assert!(idxs.iter().all(|&i| 1 <= i && i <= verts.len()));
    }
}

fn paint_top_red(s: &mut Scene, h: &mut History) {
    let m = s.mesh();
    let top = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - 3.0).abs() < 1e-9))
        .unwrap();
    let mid = s.active_mesh_id();
    h.execute(
        s,
        &mut SetFaceColorCommand {
            faces: vec![(mid, top)],
            color: Some([0.9, 0.1, 0.1]),
        },
    );
}

#[test]
fn test_obj_colours_become_materials() {
    let (mut s, mut h) = cube();
    paint_top_red(&mut s, &mut h);
    let tmp = TmpDir::new();
    let path = tmp.join("painted.obj");
    obj::save_obj(&s, &path).unwrap();
    let mtl = std::fs::read_to_string(path.with_extension("mtl")).unwrap();
    assert!(mtl.contains("Kd 0.9000 0.1000 0.1000"));
    assert!(mtl.contains("Kd 0.9600 0.9500 0.9250"));
    let (_v, faces) = read_obj(&path);
    let used: std::collections::HashSet<Option<String>> =
        faces.into_iter().map(|(m, _)| m).collect();
    assert_eq!(used.len(), 2);
}

#[test]
fn test_obj_round_trip_rebuilds_clean_solid() {
    let (s, _h) = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.obj");
    obj::save_obj(&s, &path).unwrap();
    let mut loaded = Scene::new();
    obj::load_obj(&mut loaded, &path, None, 1.0, None).unwrap();
    let m = loaded.mesh_mut();
    assert!(m.faces.len() == 6 && m.verts.len() == 8);
    assert!(is_closed(m));
    assert!((signed_volume(m) - 48.0).abs() < 1e-6);
    assert!(orient_outward(m, DEFAULT_SEED, None).is_empty());
}

#[test]
fn test_obj_import_restores_colour() {
    let (mut s, mut h) = cube();
    paint_top_red(&mut s, &mut h);
    let tmp = TmpDir::new();
    let path = tmp.join("painted.obj");
    obj::save_obj(&s, &path).unwrap();
    let mut loaded = Scene::new();
    obj::load_obj(&mut loaded, &path, None, 1.0, None).unwrap();
    let m = loaded.mesh();
    let painted: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face(f).attrs.paint.color.is_some())
        .collect();
    assert_eq!(painted.len(), 1);
    assert_eq!(m.face(painted[0]).attrs.paint.color, Some([0.9, 0.1, 0.1]));
    assert_eq!(
        m.face_ids()
            .iter()
            .filter(|&&f| m.face(f).attrs.paint.color.is_none())
            .count(),
        5
    );
}

fn big_obj(tmp: &TmpDir, n: usize) -> std::path::PathBuf {
    let mut lines = Vec::new();
    for i in 0..n {
        let x = (i * 2) as f64;
        lines.push(format!("v {x} 0 0"));
        lines.push(format!("v {} 0 0", x + 1.0));
        lines.push(format!("v {x} 1 0"));
    }
    for i in 0..n {
        lines.push(format!("f {} {} {}", 3 * i + 1, 3 * i + 2, 3 * i + 3));
    }
    let p = tmp.join("big.obj");
    std::fs::write(&p, lines.join("\n") + "\n").unwrap();
    p
}

#[test]
fn test_big_import_lands_as_reference_group() {
    let tmp = TmpDir::new();
    let n = MAX_FUSE_LOOPS + 50;
    let p = big_obj(&tmp, n);
    let mut s = Scene::new();
    let t0 = std::time::Instant::now();
    obj::load_obj(&mut s, &p, None, 1.0, None).unwrap();
    assert!(t0.elapsed().as_secs_f64() < 10.0);
    assert!(s.mesh().faces.is_empty());
    assert_eq!(s.doc.groups.len(), 1);
    assert_eq!(s.doc.mesh(s.doc.groups[0].mesh).faces.len(), n);
}

#[test]
fn test_big_import_undo_removes_the_group() {
    let tmp = TmpDir::new();
    let p = big_obj(&tmp, MAX_FUSE_LOOPS + 10);
    let mut s = Scene::new();
    let mut h = History::new();
    let mut cmd = FnCommand::new("SnapshotImport", move |s: &mut Scene| {
        obj::load_obj(s, &p, None, 1.0, None)
            .map_err(|e| xtrude_model::history::CmdError::msg(e.to_string()))
    });
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    assert_eq!(s.doc.groups.len(), 1);
    assert!(h.undo(&mut s));
    assert!(s.doc.groups.is_empty());
    assert!(h.redo(&mut s));
    assert_eq!(s.doc.groups.len(), 1);
}
