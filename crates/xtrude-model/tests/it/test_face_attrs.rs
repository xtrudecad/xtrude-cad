// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Face.attrs survive the engine's face churn (region inheritance).

use crate::common;
use common::*;
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::orient::is_closed;
use xtrude_core::Mesh;
use xtrude_model::commands::{FlipFacesCommand, MoveVerticesCommand};

fn red() -> FaceAttrs {
    let mut a = FaceAttrs::default();
    a.paint.mat = Some("red".into());
    a
}

fn blue() -> FaceAttrs {
    let mut a = FaceAttrs::default();
    a.paint.mat = Some("blue".into());
    a
}

fn top(s: &xtrude_model::Scene, z: f64) -> xtrude_core::FaceId {
    find_face(s, |p| p.iter().all(|v| (v.z - z).abs() < 1e-6)).unwrap()
}

#[test]
fn test_attrs_survive_prism_extend() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let t = top(&s, 3.0);
    s.mesh_mut().face_mut(t).attrs = red();
    push(&mut s, &mut h, t, 1.0, false);
    assert_eq!(s.mesh().face(top(&s, 4.0)).attrs, red());
}

#[test]
fn test_attrs_survive_extrude_path_repush() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1., 1., 3.),
            V3(2., 1., 3.),
            V3(2., 2., 3.),
            V3(1., 2., 3.),
        ],
    );
    let block = find_face(&s, |p| {
        p.len() == 4
            && p.iter().all(|v| (v.z - 3.0).abs() < 1e-6)
            && p.iter().map(|v| v.x).fold(f64::MIN, f64::max) <= 2.001
    })
    .unwrap();
    assert!(s.mesh().face(block).holes.is_empty());
    s.mesh_mut().face_mut(block).attrs = red();
    let n = s.mesh().face_normal(block);
    push(
        &mut s,
        &mut h,
        block,
        if n.z > 0.0 { 1.0 } else { -1.0 },
        false,
    );
    assert_eq!(s.mesh().face(top(&s, 4.0)).attrs, red());
}

#[test]
fn test_attrs_survive_corner_notch_through_floor() {
    let (mut s, mut h) = cube_scene(10.0, 3.0);
    let floor = find_face(&s, |p| p.iter().all(|v| v.z.abs() < 1e-6)).unwrap();
    s.mesh_mut().face_mut(floor).attrs = red();
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(0., 0., 3.),
            V3(4., 0., 3.),
            V3(4., 4., 3.),
            V3(0., 4., 3.),
        ],
    );
    let cface = find_face(&s, |p| {
        p.len() == 4
            && p.iter().all(|v| (v.z - 3.0).abs() < 1e-6)
            && p.iter().map(|v| v.x).fold(f64::MIN, f64::max) <= 4.001
            && p.iter().map(|v| v.y).fold(f64::MIN, f64::max) <= 4.001
    })
    .unwrap();
    let n = s.mesh().face_normal(cface);
    push(
        &mut s,
        &mut h,
        cface,
        if n.z > 0.0 { -99.0 } else { 99.0 },
        false,
    );
    let floors = faces_where(&s, |v| v.z.abs() < 1e-6);
    assert_eq!(floors.len(), 1);
    assert_eq!(s.mesh().face(floors[0]).outer.len(), 6);
    assert_eq!(s.mesh().face(floors[0]).attrs, red());
}

#[test]
fn test_attrs_survive_bump_flush_dissolve() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let wall = find_face(&s, |p| p.iter().all(|v| v.y.abs() < 1e-6)).unwrap();
    s.mesh_mut().face_mut(wall).attrs = red();
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1., 0., 1.),
            V3(2., 0., 1.),
            V3(2., 0., 2.),
            V3(1., 0., 2.),
        ],
    );
    let rect = find_face(&s, |p| {
        let c = centroid(p);
        p.len() == 4 && p.iter().all(|v| v.y.abs() < 1e-6) && 0.9 < c.x && c.x < 2.1
    })
    .unwrap();
    let n = s.mesh().face_normal(rect);
    push(
        &mut s,
        &mut h,
        rect,
        if n.y < 0.0 { 1.0 } else { -1.0 },
        false,
    );
    let cap = find_face(&s, |p| p.iter().all(|v| (v.y + 1.0).abs() < 1e-6)).unwrap();
    let n = s.mesh().face_normal(cap);
    push(
        &mut s,
        &mut h,
        cap,
        if n.y < 0.0 { -1.0 } else { 1.0 },
        false,
    );
    let m = s.mesh();
    assert!(is_closed(m) && m.faces.len() == 6);
    let healed = find_face(&s, |p| p.iter().all(|v| v.y.abs() < 1e-6)).unwrap();
    assert_eq!(s.mesh().face(healed).attrs, red());
}

#[test]
fn test_attrs_survive_autofold() {
    let (mut s, mut h) = scene_history();
    let f = s
        .mesh_mut()
        .add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    s.mesh_mut().face_mut(f).attrs = red();
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V(2., 2.)],
            delta: V3(0., 0., 1.),
        },
    );
    let m = s.mesh();
    assert_eq!(m.faces.len(), 2);
    assert!(m.face_ids().iter().all(|&f| m.face(f).attrs == red()));
}

#[test]
fn test_attrs_survive_dedupe() {
    let mut m = Mesh::new();
    let outer = [V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)];
    m.add_face(&outer, &[]);
    let dup = m.add_face(&outer, &[]);
    m.face_mut(dup).attrs = red();
    assert_eq!(m.dedupe_faces(), 1);
    let survivor = m.face_ids()[0];
    assert_eq!(m.face(survivor).attrs, red());
}

#[test]
fn test_attrs_survive_coplanar_dissolve() {
    let mut m = Mesh::new();
    let big = m.add_face(&[V(0., 0.), V(3., 0.), V(3., 2.), V(0., 2.)], &[]);
    let small = m.add_face(&[V(3., 0.), V(4., 0.), V(4., 2.), V(3., 2.)], &[]);
    m.face_mut(big).attrs = red();
    m.face_mut(small).attrs = blue();
    let merged = m.dissolve_coplanar_region(&[big, small]).unwrap();
    assert_eq!(m.face(merged).attrs, red());
}

#[test]
fn test_attrs_survive_snapshot_roundtrip() {
    let (mut s, _h) = cube_scene(4.0, 3.0);
    let t = top(&s, 3.0);
    s.mesh_mut().face_mut(t).attrs = red();
    let snap = s.mesh().clone();
    s.mesh_mut().face_mut(t).attrs = blue();
    *s.mesh_mut() = snap;
    assert_eq!(s.mesh().face(t).attrs, red());
}

#[test]
fn test_flip_faces_command_reverses_winding_and_undoes() {
    let (mut s, mut h) = scene_history();
    let f = s
        .mesh_mut()
        .add_face(&[V(0., 0.), V(1., 0.), V(1., 1.)], &[]);
    let mid = s.active_mesh_id();
    assert!(s.mesh().face_normal(f).z > 0.0);
    h.run(
        &mut s,
        FlipFacesCommand {
            faces: vec![(mid, f)],
        },
    );
    assert!(s.mesh().face_normal(f).z < 0.0);
    assert!(s.mesh().has_face(f));
    h.undo(&mut s);
    assert!(s.mesh().face_normal(f).z > 0.0);
    h.redo(&mut s);
    assert!(s.mesh().face_normal(f).z < 0.0);
}
