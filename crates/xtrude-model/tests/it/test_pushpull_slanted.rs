// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The slanted-faces bench.

use crate::common;
use common::*;
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::triangulate::plane_axes;
use xtrude_core::{DVec3, FaceId};
use xtrude_model::commands::MoveVerticesCommand;
use xtrude_model::edits::build_add_edge;
use xtrude_model::{History, Scene};

fn gable_prism() -> (Scene, History) {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let c = build_add_edge(&s, V3(2., 0., 3.), V3(2., 4., 3.), true, None);
    exec(&mut s, &mut h, c);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V3(2., 0., 3.), V3(2., 4., 3.)],
            delta: V3(0., 0., 1.),
        },
    );
    (s, h)
}

fn wedge() -> (Scene, History) {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V3(0., 0., 3.), V3(0., 4., 3.)],
            delta: V3(0., 0., -2.),
        },
    );
    (s, h)
}

fn assert_clean(s: &Scene, ctx: &str) {
    let m = s.mesh();
    assert!(is_closed(m), "{ctx}: solid left open");
    assert!(signed_volume(m) > 0.0, "{ctx}: bad signed volume");
    assert!(
        orient_outward(&mut m.clone(), DEFAULT_SEED, None).is_empty(),
        "{ctx}: inconsistent winding"
    );
    for (_, e) in m.edges.iter() {
        if e.faces.len() != 2 {
            continue;
        }
        let d = m
            .face_normal(e.faces[0])
            .normalize()
            .dot(m.face_normal(e.faces[1]).normalize());
        assert!(d.abs() <= 0.999, "{ctx}: unmerged coplanar seam");
    }
}

fn slope(s: &Scene) -> FaceId {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z > 0.5 && m.face_normal(f).x > 0.1)
        .unwrap()
}

fn gable_face(s: &Scene) -> FaceId {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face(f).outer.len() == 5 && m.face_normal(f).y < -0.5)
        .unwrap()
}

#[test]
fn test_gable_prism_builds_clean() {
    let (s, _h) = gable_prism();
    assert_clean(&s, "gable build");
    assert_eq!(nfaces(&s), 7);
    assert!((signed_volume(s.mesh()) - 56.0).abs() < 1e-6);
}

#[test]
fn test_thicken_slope_plane() {
    let (mut s, mut h) = gable_prism();
    let r = slope(&s);
    let area = s.mesh().face_area(r);
    push(&mut s, &mut h, r, 0.3, false);
    assert_clean(&s, "thicken slope");
    assert!((signed_volume(s.mesh()) - (56.0 + 0.3 * area)).abs() < 1e-3);
}

#[test]
fn test_slope_pushed_back_flush_restores_prism() {
    let (mut s, mut h) = gable_prism();
    let r = slope(&s);
    push(&mut s, &mut h, r, 0.3, false);
    let raised = slope(&s);
    push(&mut s, &mut h, raised, -0.3, false);
    assert_clean(&s, "slope flush back");
    assert!((signed_volume(s.mesh()) - 56.0).abs() < 1e-3);
    assert_eq!(nfaces(&s), 7);
}

#[test]
fn test_extend_gable_pentagon() {
    let (mut s, mut h) = gable_prism();
    let g = gable_face(&s);
    push(&mut s, &mut h, g, 1.0, false);
    assert_clean(&s, "extend gable");
    assert!((signed_volume(s.mesh()) - 70.0).abs() < 1e-6);
    assert_eq!(nfaces(&s), 7);
}

fn slope_rect(s: &Scene) -> (DVec3, Vec<DVec3>) {
    let r = slope(s);
    let n = s.mesh().face_normal(r).normalize();
    let (u, w) = plane_axes(n);
    let o = s.mesh().face_centroid(r);
    (
        n,
        vec![
            o - u * 0.5 - w * 0.5,
            o + u * 0.5 - w * 0.5,
            o + u * 0.5 + w * 0.5,
            o - u * 0.5 + w * 0.5,
        ],
    )
}

#[test]
fn test_recess_on_slanted_slope() {
    let (mut s, mut h) = gable_prism();
    let (n, rect) = slope_rect(&s);
    rectangle(&mut s, &mut h, &rect);
    let m = s.mesh();
    let sub = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            m.face(f).outer.len() == 4
                && m.face(f).holes.is_empty()
                && m.face_area(f) < 1.1
                && m.face_normal(f).normalize().dot(n).abs() > 0.999
        })
        .unwrap();
    push(&mut s, &mut h, sub, -0.2, false);
    assert_clean(&s, "recess on slope");
    assert!((signed_volume(s.mesh()) - (56.0 - 0.2)).abs() < 1e-3);
}

#[test]
fn test_recess_attrs_survive_on_slanted_slope() {
    let (mut s, mut h) = gable_prism();
    let r = slope(&s);
    let mut tile = FaceAttrs::default();
    tile.paint.mat = Some("tile".into());
    s.mesh_mut().face_mut(r).attrs = tile.clone();
    let (n, rect) = slope_rect(&s);
    rectangle(&mut s, &mut h, &rect);
    let m = s.mesh();
    let host = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty() && m.face_normal(f).normalize().dot(n).abs() > 0.999)
        .unwrap();
    assert_eq!(m.face(host).attrs, tile);
}

fn wedge_faces(s: &Scene) -> Vec<FaceId> {
    let m = s.mesh();
    let cap = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z.abs() > 0.3 && m.face_centroid(f).z > 1.0)
        .unwrap();
    let mut walls: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_normal(f).z.abs() < 0.1 && m.face_centroid(f).z > 0.0 && f != cap)
        .collect();
    let r3 = |x: f64| (x * 1000.0).round() as i64;
    walls.sort_by_key(|&f| (r3(m.face_centroid(f).x), r3(m.face_centroid(f).y)));
    let mut out = vec![cap];
    out.extend(walls);
    out
}

#[test]
fn test_wedge_builds_clean() {
    let (s, _h) = wedge();
    assert_clean(&s, "wedge build");
    assert!((signed_volume(s.mesh()) - 32.0).abs() < 1e-6);
}

#[test]
fn test_wedge_push_orders() {
    let dists = [0.7, 1.1, 0.5];
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for perm in perms {
        let (mut s, mut h) = wedge();
        for (k, &fi) in perm.iter().enumerate() {
            let faces = wedge_faces(&s);
            push(&mut s, &mut h, faces[fi], dists[k], false);
            assert_clean(&s, &format!("wedge perm={perm:?} step={k}"));
        }
    }
}

#[test]
fn test_wedge_single_face_in_and_out() {
    for fi in 0..4 {
        let (mut s, mut h) = wedge();
        let f = wedge_faces(&s)[fi];
        push(&mut s, &mut h, f, 0.6, false);
        assert_clean(&s, &format!("wedge out fi={fi}"));
        let (mut s, mut h) = wedge();
        let f = wedge_faces(&s)[fi];
        push(&mut s, &mut h, f, -0.4, false);
        assert_clean(&s, &format!("wedge in fi={fi}"));
    }
}

#[test]
fn test_slanted_undo_redo_roundtrip() {
    let (mut s, mut h) = gable_prism();
    let r = slope(&s);
    push(&mut s, &mut h, r, 0.3, false);
    let after = nfaces(&s);
    assert!(h.undo(&mut s));
    assert_eq!(nfaces(&s), 7);
    assert!(h.redo(&mut s));
    assert_eq!(nfaces(&s), after);
    assert_clean(&s, "undo/redo roundtrip");
}
