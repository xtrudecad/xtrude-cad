// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Named regressions for engine bugs the fuzz bench found.

use crate::common;
use common::*;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::topology::{is_planar, PLANAR_TOLERANCE};
use xtrude_core::{FaceId, Mesh};

fn wall_rect(s: &mut xtrude_model::Scene, h: &mut xtrude_model::History) -> FaceId {
    rectangle(
        s,
        h,
        &[
            V3(1., 0., 1.),
            V3(2., 0., 1.),
            V3(2., 0., 2.),
            V3(1., 0., 2.),
        ],
    );
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| {
            let p = m.face_positions(f);
            let c = centroid(&p);
            p.len() == 4
                && m.face(f).holes.is_empty()
                && p.iter().all(|v| v.y.abs() < 1e-9)
                && 0.9 < c.x
                && c.x < 2.1
                && 0.9 < c.z
                && c.z < 2.1
        })
        .unwrap()
}

fn orient_clean(m: &Mesh) -> bool {
    orient_outward(&mut m.clone(), DEFAULT_SEED, None).is_empty()
}

#[test]
fn test_push_holed_wall_extrudes_hole_rim_too() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    wall_rect(&mut s, &mut h);
    let wall = s
        .mesh()
        .face_ids()
        .into_iter()
        .find(|&f| !s.mesh().face(f).holes.is_empty())
        .unwrap();
    let n = s.mesh().face_normal(wall);
    assert!(push(
        &mut s,
        &mut h,
        wall,
        if n.y < 0.0 { 1.0 } else { -1.0 },
        false
    ));
    let m = s.mesh();
    assert!(is_closed(m));
    assert!(m
        .face_ids()
        .iter()
        .all(|&f| is_planar(&m.face_positions(f), PLANAR_TOLERANCE)));
    assert!(signed_volume(m) > 0.0);
    assert!(orient_clean(m));
}

#[test]
fn test_bump_side_flank_pushed_flush_dissolves_bump() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let rect = wall_rect(&mut s, &mut h);
    let n = s.mesh().face_normal(rect);
    push(
        &mut s,
        &mut h,
        rect,
        if n.y < 0.0 { 1.0 } else { -1.0 },
        false,
    );
    let flank = find_face(&s, |p| {
        p.iter().all(|v| (v.x - 2.0).abs() < 1e-9) && centroid(p).y < -0.01
    })
    .unwrap();
    assert!(push(&mut s, &mut h, flank, -1.0, false));
    let m = s.mesh();
    assert_eq!((m.faces.len(), m.edges.len(), m.verts.len()), (6, 12, 8));
    assert!(is_closed(m) && (signed_volume(m) - 48.0).abs() < 1e-6);
    assert!(h.undo(&mut s) && h.redo(&mut s));
}

#[test]
fn test_push_below_weld_tolerance_is_noop() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let top = find_face(&s, |p| p.iter().all(|v| (v.z - 3.0).abs() < 1e-9)).unwrap();
    let before = h.undo_len();
    let pushed = push(&mut s, &mut h, top, 5e-5, false);
    assert!(!pushed || h.undo_len() == before);
    assert_eq!(nfaces(&s), 6);
}

#[test]
fn test_draw_on_shared_plane_does_not_flip_the_other_solid() {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(3., 0.), V(3., 4.), V(0., 4.)],
    );
    rectangle(
        &mut s,
        &mut h,
        &[V(3., 0.), V(6., 0.), V(6., 4.), V(3., 4.)],
    );
    let m = s.mesh();
    let room_a = m
        .face_ids()
        .into_iter()
        .min_by(|&a, &b| {
            m.face_centroid(a)
                .x
                .partial_cmp(&m.face_centroid(b).x)
                .unwrap()
        })
        .unwrap();
    let d = up(&s, room_a, 2.7);
    push(&mut s, &mut h, room_a, d, false);
    let room_b = find_face(&s, |p| {
        p.iter().all(|v| v.z.abs() < 1e-9) && centroid(p).x > 3.0
    })
    .unwrap();
    let d = up(&s, room_b, 2.7);
    push(&mut s, &mut h, room_b, d, false);
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(6., 1., 1.),
            V3(6., 2., 1.),
            V3(6., 2., 2.),
            V3(6., 1., 2.),
        ],
    );
    assert!(orient_clean(s.mesh()));
}

#[test]
fn test_dedupe_same_outer_cycle_keeps_the_subdivided_face() {
    let mut m = Mesh::new();
    let outer = [V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)];
    let hole = vec![V(1., 1.), V(2., 1.), V(2., 2.), V(1., 2.)];
    let holed = m.add_face(&outer, std::slice::from_ref(&hole));
    m.add_face(&hole, &[]);
    m.add_face(&outer, &[]);
    assert_eq!(m.dedupe_faces(), 1);
    assert!(m.has_face(holed));
    assert_eq!(m.faces.len(), 2);
}

#[test]
fn test_nested_rectangle_punches_its_mother_not_grandmother() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1., 1., 3.),
            V3(3., 1., 3.),
            V3(3., 3., 3.),
            V3(1., 3., 3.),
        ],
    );
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1.5, 1.5, 3.),
            V3(2.5, 1.5, 3.),
            V3(2.5, 2.5, 3.),
            V3(1.5, 2.5, 3.),
        ],
    );
    let m = s.mesh();
    assert!(is_closed(m));
    let rect_a = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            !m.face(f).holes.is_empty()
                && m.face_positions(f)
                    .iter()
                    .map(|v| v.x)
                    .fold(f64::INFINITY, f64::min)
                    > 0.9
        })
        .unwrap();
    assert_eq!(m.face(rect_a).holes.len(), 1);
}

#[test]
fn test_ctrl_inward_stack_splits_boundary_at_belt() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let wall = find_face(&s, |p| p.iter().all(|v| v.y.abs() < 1e-9)).unwrap();
    let n = s.mesh().face_normal(wall);
    push(
        &mut s,
        &mut h,
        wall,
        if n.y < 0.0 { -0.5 } else { 0.5 },
        true,
    );
    let m = s.mesh();
    assert_eq!(m.faces.len(), 11);
    assert!(is_closed(m));
    assert_eq!(
        m.face_ids().iter().filter(|&&f| m.face(f).interior).count(),
        1
    );
    for f in m.face_ids() {
        let ks: Vec<_> = m
            .face_positions(f)
            .iter()
            .map(|v| {
                (
                    (v.x * 1e6).round() as i64,
                    (v.y * 1e6).round() as i64,
                    (v.z * 1e6).round() as i64,
                )
            })
            .collect();
        let u: std::collections::HashSet<_> = ks.iter().collect();
        assert_eq!(ks.len(), u.len());
    }
    assert!(orient_clean(m));
}

#[test]
fn test_two_ctrl_stacks_stay_closed() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let front = find_face(&s, |p| p.iter().all(|v| v.y.abs() < 1e-9)).unwrap();
    let n = s.mesh().face_normal(front);
    push(
        &mut s,
        &mut h,
        front,
        if n.y < 0.0 { 2.2 } else { -2.2 },
        true,
    );
    let left = find_face(&s, |p| {
        let c = centroid(p);
        p.iter().all(|v| v.x.abs() < 1e-9) && -0.1 < c.y && c.y < 4.1 && p.len() == 4 && c.y > 1.0
    })
    .unwrap();
    let n = s.mesh().face_normal(left);
    push(
        &mut s,
        &mut h,
        left,
        if n.x < 0.0 { -1.6 } else { 1.6 },
        true,
    );
    assert!(is_closed(s.mesh()));
    assert!(orient_clean(s.mesh()));
}

#[test]
fn test_ctrl_stack_into_body_keeps_its_new_divisions() {
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
    let rect = find_face(&s, |p| {
        p.len() == 4
            && p.iter().all(|v| (v.z - 3.0).abs() < 1e-9)
            && p.iter().map(|v| v.x).fold(f64::NEG_INFINITY, f64::max) <= 2.001
    })
    .unwrap();
    let n = s.mesh().face_normal(rect);
    push(
        &mut s,
        &mut h,
        rect,
        if n.z > 0.0 { -1.0 } else { 1.0 },
        false,
    );
    let wall = find_face(&s, |p| {
        p.iter().all(|v| (v.x - 2.0).abs() < 1e-9) && centroid(p).z > 2.0
    })
    .unwrap();
    let n = s.mesh().face_normal(wall);
    push(
        &mut s,
        &mut h,
        wall,
        if n.x > 0.0 { -0.5 } else { 0.5 },
        true,
    );
    assert!(is_closed(s.mesh()));
    assert!(orient_clean(s.mesh()));
}

#[test]
fn test_partition_survives_push_on_another_plane() {
    let (mut s, mut h) = cube_scene(4.0, 3.0);
    let wall = find_face(&s, |p| p.iter().all(|v| v.x.abs() < 1e-9)).unwrap();
    push(&mut s, &mut h, wall, 1.5, true);
    let bottom = find_face(&s, |p| {
        p.iter().all(|v| v.z.abs() < 1e-9) && centroid(p).x > 0.0
    })
    .unwrap();
    let n = s.mesh().face_normal(bottom);
    push(
        &mut s,
        &mut h,
        bottom,
        if n.z < 0.0 { -1.0 } else { 1.0 },
        false,
    );
    let m = s.mesh();
    assert!(is_closed(m), "crack: partition cleanup broke the shell");
    let division = m
        .face_ids()
        .into_iter()
        .any(|f| m.face_positions(f).iter().all(|v| v.x.abs() < 1e-9) && m.face(f).interior);
    assert!(division, "the kept dividing face was dissolved");
}

#[test]
fn test_inward_push_clamps_at_lateral_exit() {
    let (mut s, mut h) = scene_history();
    let tri = [V(-0.2, -2.8), V(2.7, -7.2), V(4.1, -2.7)];
    s.mesh_mut().add_face(&tri, &[]);
    let f = s.mesh().face_ids()[0];
    let d = up(&s, f, 3.0);
    push(&mut s, &mut h, f, d, false);
    let m = s.mesh();
    let wall = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z.abs() < 0.1 && m.face_centroid(f).y > -3.0)
        .unwrap();
    push(&mut s, &mut h, wall, -2.48, false);
    let m = s.mesh();
    assert!(is_closed(m));
    assert!(signed_volume(m) > 0.0);
    assert!(orient_clean(m));
}
