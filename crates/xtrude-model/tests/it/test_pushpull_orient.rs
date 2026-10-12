// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Push/pull on a solid commits a consistently oriented watertight mesh —
//! closed, seam-free, positive volume, nothing for `orient_outward` to flip —
//! in every push order.

use crate::common;

use common::*;
use xtrude_core::math::VecExt;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::history::History;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;
use xtrude_model::scene::Scene;

/// The irregular triangle extruded into a prism.
fn prism_solid() -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    let f = s
        .mesh_mut()
        .add_face(&[V(-0.2, -2.8), V(2.7, -7.2), V(4.1, -2.7)], &[]);
    assert!(push_face(&mut s, &mut h, MeshId::LOOSE, f, 3.0, false));
    (s, h)
}

/// The vertical walls, in a stable order.
fn side_walls(s: &Scene) -> Vec<FaceId> {
    let m = s.mesh();
    let mut w: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_normal(f).z.abs() < 0.1)
        .collect();
    w.sort_by_key(|&f| {
        let c = m.face_centroid(f);
        ((c.x * 1000.0).round() as i64, (c.y * 1000.0).round() as i64)
    });
    w
}

fn unmerged_coplanar_seams(m: &Mesh) -> usize {
    m.edges
        .iter()
        .filter(|(_, e)| {
            e.faces.len() == 2
                && m.face_normal(e.faces[0])
                    .unit_or_zero()
                    .dot(m.face_normal(e.faces[1]).unit_or_zero())
                    .abs()
                    > 0.999
        })
        .count()
}

fn assert_clean_solid(s: &mut Scene, what: &str) {
    let m = s.mesh_mut();
    assert!(is_closed(m), "{what}: solid left open");
    assert_eq!(unmerged_coplanar_seams(m), 0, "{what}: coplanar seam");
    assert!(signed_volume(m) > 0.0, "{what}: winding");
    assert!(
        orient_outward(m, DEFAULT_SEED, None).is_empty(),
        "{what}: inconsistent winding"
    );
}

const DISTS: [f64; 3] = [1.5, 1.0, 2.0];

fn push_walls(seq: &[usize]) -> Scene {
    let (mut s, mut h) = prism_solid();
    for (k, &wi) in seq.iter().enumerate() {
        let f = side_walls(&s)[wi];
        push_face(
            &mut s,
            &mut h,
            MeshId::LOOSE,
            f,
            DISTS[k % DISTS.len()],
            false,
        );
    }
    s
}

#[test]
fn test_side_wall_push_orders_commit_oriented_solid() {
    let mut seqs: Vec<Vec<usize>> = Vec::new();
    for a in 0..3 {
        seqs.push(vec![a]);
        for b in (0..3).filter(|&b| b != a) {
            seqs.push(vec![a, b]);
            for c in (0..3).filter(|&c| c != a && c != b) {
                seqs.push(vec![a, b, c]);
            }
        }
    }
    assert_eq!(seqs.len(), 15);
    for seq in seqs {
        let mut s = push_walls(&seq);
        assert_clean_solid(&mut s, &format!("{seq:?}"));
    }
}

#[test]
fn test_repeated_wall_push_commits_oriented_solid() {
    for seq in [[0, 0, 1], [1, 1, 2], [2, 2, 0], [0, 1, 0]] {
        let mut s = push_walls(&seq);
        assert_clean_solid(&mut s, &format!("{seq:?}"));
    }
}

#[test]
fn test_plain_prism_is_oriented() {
    let (mut s, _h) = prism_solid();
    assert_clean_solid(&mut s, "prism");
}

#[test]
fn test_push_undo_redo_round_trip() {
    let (mut s, mut h) = prism_solid();
    let f = side_walls(&s)[0];
    push_face(&mut s, &mut h, MeshId::LOOSE, f, 1.5, false);
    let after = s.mesh().face_ids().len();
    assert!(h.undo(&mut s));
    assert!(h.redo(&mut s));
    assert_eq!(s.mesh().face_ids().len(), after);
    assert_clean_solid(&mut s, "redo");
}
