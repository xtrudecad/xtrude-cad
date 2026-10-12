// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A line the user drew across a face is a decision: rebuilds keep it, while
//! machine seams dissolve.

use crate::common;
use common::*;
use xtrude_core::orient::{is_closed, orient_outward, DEFAULT_SEED};
use xtrude_core::{DVec3, EdgeId, FaceId, Mesh};
use xtrude_model::commands::{AddFaceCommand, EraseSelectionCommand};
use xtrude_model::edits::{build_add_edge, build_add_edges};
use xtrude_model::history::{Command, History};
use xtrude_model::scene::Scene;

fn key(p: DVec3) -> (i64, i64, i64) {
    (
        (p.x * 1e6).round() as i64,
        (p.y * 1e6).round() as i64,
        (p.z * 1e6).round() as i64,
    )
}

fn add_edge(s: &mut Scene, h: &mut History, a: DVec3, b: DVec3) -> bool {
    let mut c = build_add_edge(s, a, b, true, None);
    h.execute(s, &mut *c)
}

fn cube_with_chord() -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)],
    );
    let f = s.mesh().face_ids()[0];
    let d = up(&s, f, 3.0);
    push(&mut s, &mut h, f, d, false);
    add_edge(&mut s, &mut h, V3(0., 0., 1.), V3(4., 0., 2.));
    (s, h)
}

fn chord_edges(m: &Mesh) -> Vec<EdgeId> {
    let want = [key(V3(0., 0., 1.)), key(V3(4., 0., 2.))];
    m.edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            let mut k = [key(a), key(b)];
            k.sort();
            let mut w = want;
            w.sort();
            k == w
        })
        .collect()
}

fn on_y0(m: &Mesh, f: FaceId) -> bool {
    m.face_positions(f).iter().all(|p| p.y.abs() < 1e-9)
}

fn closed_and_oriented(s: &Scene) -> bool {
    let mut m = s.mesh().clone();
    is_closed(&m) && orient_outward(&mut m, DEFAULT_SEED, None).is_empty()
}

fn y_out(m: &Mesh, f: FaceId, d: f64) -> f64 {
    if m.face_normal(f).y < 0.0 {
        d
    } else {
        -d
    }
}

#[test]
fn test_chord_splits_the_wall() {
    let (s, _h) = cube_with_chord();
    let m = s.mesh();
    let walls = m.face_ids().into_iter().filter(|&f| on_y0(m, f)).count();
    assert_eq!(walls, 2);
    assert_eq!(chord_edges(m).len(), 1);
}

#[test]
fn test_chord_survives_bump_on_same_wall() {
    let (mut s, mut h) = cube_with_chord();
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1., 0., 0.2),
            V3(2., 0., 0.2),
            V3(2., 0., 0.8),
            V3(1., 0., 0.8),
        ],
    );
    let m = s.mesh();
    let rect = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            m.face(f).outer.len() == 4
                && m.face(f).holes.is_empty()
                && on_y0(m, f)
                && m.face_area(f) < 0.7
        })
        .unwrap();
    let d = y_out(m, rect, 0.5);
    push(&mut s, &mut h, rect, d, false);
    assert_eq!(
        chord_edges(s.mesh()).len(),
        1,
        "user chord dissolved by the rebuild"
    );
    assert!(closed_and_oriented(&s));
}

fn top_at(m: &Mesh, z: f64) -> FaceId {
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - z).abs() < 1e-9))
        .unwrap()
}

#[test]
fn test_chord_survives_ctrl_stack_on_top() {
    let (mut s, mut h) = cube_with_chord();
    let top = top_at(s.mesh(), 3.0);
    push(&mut s, &mut h, top, 1.0, true);
    assert_eq!(
        chord_edges(s.mesh()).len(),
        1,
        "user chord dissolved by the Ctrl stack"
    );
    assert!(closed_and_oriented(&s));
}

#[test]
fn test_stacked_strip_seam_still_dissolves() {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)],
    );
    let f = s.mesh().face_ids()[0];
    let d = up(&s, f, 3.0);
    push(&mut s, &mut h, f, d, false);
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
    let m = s.mesh();
    let rect = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            let c = m.face_centroid(f).x;
            m.face(f).outer.len() == 4
                && m.face(f).holes.is_empty()
                && on_y0(m, f)
                && 0.9 < c
                && c < 2.1
        })
        .unwrap();
    let d = y_out(m, rect, 0.5);
    push(&mut s, &mut h, rect, d, false);
    let m = s.mesh();
    let cap = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.y + 0.5).abs() < 1e-6))
        .unwrap();
    let d = y_out(m, cap, 0.5);
    push(&mut s, &mut h, cap, d, false);
    let m = s.mesh();
    let flanks = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            m.face_positions(f).iter().all(|p| (p.x - 2.0).abs() < 1e-9)
                && m.face_centroid(f).y < -0.01
        })
        .count();
    assert_eq!(flanks, 1, "the stacked strip seam did not dissolve");
    assert!(is_closed(s.mesh()));
}

#[test]
fn test_chord_attrs_partition_survives_push() {
    let (mut s, mut h) = cube_with_chord();
    let (lower_color, upper_color) = ([0.3, 0.2, 0.1], [0.9, 0.9, 0.8]);
    {
        let m = s.mesh_mut();
        let lower = m
            .face_ids()
            .into_iter()
            .find(|&f| on_y0(m, f) && m.face_centroid(f).z < 1.4)
            .unwrap();
        let upper = m
            .face_ids()
            .into_iter()
            .find(|&f| on_y0(m, f) && f != lower)
            .unwrap();
        m.face_mut(lower).attrs.paint.color = Some(lower_color);
        m.face_mut(upper).attrs.paint.color = Some(upper_color);
    }
    let top = top_at(s.mesh(), 3.0);
    push(&mut s, &mut h, top, 1.0, true);
    let m = s.mesh();
    let colors: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| on_y0(m, f))
        .filter_map(|f| m.face(f).attrs.paint.color)
        .collect();
    assert!(colors.contains(&lower_color) && colors.contains(&upper_color));
}

fn slab_with_inner_rect() -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(6., 0.), V(6., -4.), V(0., -4.)],
    );
    let f = s.mesh().face_ids()[0];
    let d = up(&s, f, 1.0);
    push(&mut s, &mut h, f, d, false);
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(2., -1., 1.),
            V3(4., -1., 1.),
            V3(4., -3., 1.),
            V3(2., -3., 1.),
        ],
    );
    (s, h)
}

fn top_faces(m: &Mesh) -> Vec<FaceId> {
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.z - 1.0).abs() < 1e-6))
        .collect()
}

#[test]
fn test_line_grazing_hole_boundary_subdivides_not_stacks() {
    let (mut s, mut h) = slab_with_inner_rect();
    add_edge(&mut s, &mut h, V3(2., 0., 1.), V3(2., -4., 1.));
    assert!(h.last_error.is_none());
    let m = s.mesh();
    let tops = top_faces(m);
    let mut areas: Vec<f64> = tops
        .iter()
        .map(|&f| (m.face_area(f) * 100.0).round() / 100.0)
        .collect();
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(areas, [4.0, 8.0, 12.0]);
    assert!(
        tops.iter().all(|&f| m.face_normal(f).z > 0.99),
        "a top face came out flipped"
    );
    assert!(is_closed(m));
}

#[test]
fn test_deleting_the_fence_line_fuses_the_plane_back() {
    let (mut s, mut h) = slab_with_inner_rect();
    add_edge(&mut s, &mut h, V3(2., 0., 1.), V3(2., -4., 1.));
    let m = s.mesh();
    let fence: Vec<EdgeId> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.x - 2.).abs() < 1e-6
                && (b.x - 2.).abs() < 1e-6
                && (a.z - 1.).abs() < 1e-6
                && (b.z - 1.).abs() < 1e-6
                && !(a.y.min(b.y) >= -3.0 - 1e-6 && a.y.max(b.y) <= -1.0 + 1e-6)
        })
        .collect();
    assert!(!fence.is_empty());
    let mut cmd = EraseSelectionCommand::from_entities(m, &fence, &[]);
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    let m = s.mesh();
    let big: Vec<FaceId> = top_faces(m)
        .into_iter()
        .filter(|&f| m.face_area(f) > 15.0)
        .collect();
    assert!(
        !big.is_empty(),
        "the mother top face must survive and re-fuse"
    );
    assert_eq!(m.face(big[0]).holes.len(), 1);
    assert!(is_closed(m));
}

fn rect(s: &mut Scene, h: &mut History, a: (f64, f64), c: (f64, f64)) {
    let cs = [V(a.0, a.1), V(c.0, a.1), V(c.0, c.1), V(a.0, c.1)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (cs[i], cs[(i + 1) % 4])).collect();
    let face: Box<dyn Command> = Box::new(AddFaceCommand::new(cs.to_vec()));
    let mut c = build_add_edges(s, &segs, false, vec![face], None);
    h.execute(s, &mut *c);
}

fn slit_edges(m: &Mesh) -> Vec<EdgeId> {
    m.edge_ids()
        .into_iter()
        .filter(|&e| {
            let fs = &m.edge(e).faces;
            fs.len() == 2 && fs[0] == fs[1]
        })
        .collect()
}

fn flat_ring_with_fence() -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    rect(&mut s, &mut h, (0., 0.), (30., 20.));
    rect(&mut s, &mut h, (10., 6.), (24., 14.));
    add_edge(&mut s, &mut h, V(10., 6.), V(10., 0.));
    (s, h)
}

#[test]
fn test_deleting_a_slit_edge_keeps_the_face() {
    let (mut s, mut h) = flat_ring_with_fence();
    let slits = slit_edges(s.mesh());
    assert_eq!(slits.len(), 1);
    let mut cmd = EraseSelectionCommand::from_entities(s.mesh(), &slits, &[]);
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    let m = s.mesh();
    assert!(slit_edges(m).is_empty());
    let big: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_area(f) > 400.0)
        .collect();
    assert!(
        !big.is_empty(),
        "the ring face must survive the fence deletion"
    );
    assert_eq!(m.face(big[0]).holes.len(), 1);
    assert!(h.undo(&mut s));
    assert_eq!(slit_edges(s.mesh()).len(), 1);
}

#[test]
fn test_deleting_one_piece_of_a_two_piece_fence() {
    let (mut s, mut h) = scene_history();
    rect(&mut s, &mut h, (0., 0.), (30., 20.));
    rect(&mut s, &mut h, (10., 6.), (24., 14.));
    add_edge(&mut s, &mut h, V(10., 6.), V(10., 3.));
    add_edge(&mut s, &mut h, V(10., 3.), V(10., 0.));
    let m = s.mesh();
    let lower: Vec<EdgeId> = slit_edges(m)
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            a.y.max(b.y) < 3.01
        })
        .collect();
    assert_eq!(lower.len(), 1);
    let mut cmd = EraseSelectionCommand::from_entities(m, &lower, &[]);
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    let m = s.mesh();
    let big: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_area(f) > 400.0)
        .collect();
    assert!(!big.is_empty() && m.face(big[0]).holes.len() == 1);
    let upper = m.edge_ids().into_iter().any(|e| {
        let (a, b) = m.edge_ab(e);
        m.edge(e).faces.is_empty() && (a.x - 10.).abs() < 1e-6 && (b.x - 10.).abs() < 1e-6
    });
    assert!(
        upper,
        "the untouched fence piece must survive as a free edge"
    );
}
