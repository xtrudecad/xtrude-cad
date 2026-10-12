// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Translate positions; connected geometry follows.

use crate::common;
use common::*;
use std::collections::HashSet;
use xtrude_model::commands::MoveVerticesCommand;
use xtrude_model::edits::build_add_edges;

#[test]
fn test_move_drags_all_coincident_points() {
    let (mut s, mut h) = scene_history();
    let left = s
        .mesh_mut()
        .add_face(&[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)], &[]);
    let right = s
        .mesh_mut()
        .add_face(&[V(1., 0.), V(2., 0.), V(2., 1.), V(1., 1.)], &[]);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V(1., 0.), V(1., 1.)],
            delta: V3(0., 0., 1.),
        },
    );
    let m = s.mesh();
    assert!(m
        .face_positions(left)
        .iter()
        .any(|v| (v.z - 1.0).abs() < 1e-9));
    assert!(m
        .face_positions(right)
        .iter()
        .any(|v| (v.z - 1.0).abs() < 1e-9));
    let moved: HashSet<(i64, i64)> = m
        .face_positions(left)
        .iter()
        .filter(|v| (v.z - 1.0).abs() < 1e-9)
        .map(|v| (v.x.round() as i64, v.y.round() as i64))
        .collect();
    assert_eq!(moved, HashSet::from([(1, 0), (1, 1)]));
}

#[test]
fn test_move_undo_restores_positions() {
    let (mut s, mut h) = scene_history();
    let f = s
        .mesh_mut()
        .add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V(2., 0.), V(2., 2.)],
            delta: V(1., 0.),
        },
    );
    assert!(s
        .mesh()
        .face_positions(f)
        .iter()
        .any(|v| (v.x - 3.0).abs() < 1e-9));
    assert!(h.undo(&mut s));
    let p = s.mesh().face_positions(f);
    assert!(p.iter().all(|v| (v.x - 3.0).abs() > 1e-9));
    assert_eq!(p.iter().map(|v| v.x).fold(f64::MIN, f64::max), 2.0);
}

#[test]
fn test_ridge_move_deforms_slopes() {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(4., 0.), V(4., 2.), V(0., 2.)],
    );
    s.mesh_mut().add_face(
        &[
            V3(0., 0., 2.),
            V3(4., 0., 2.),
            V3(4., 2., 2.),
            V3(0., 2., 2.),
        ],
        &[],
    );
    let c = build_add_edges(&s, &[(V3(0., 1., 2.), V3(4., 1., 2.))], true, vec![], None);
    exec(&mut s, &mut h, c);
    let m = s.mesh();
    let halves: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|v| (v.z - 2.0).abs() < 1e-9))
        .collect();
    assert_eq!(halves.len(), 2);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V3(0., 1., 2.), V3(4., 1., 2.)],
            delta: V3(0., 0., 1.),
        },
    );
    for f in halves {
        let p = s.mesh().face_positions(f);
        assert!(p.iter().any(|v| (v.z - 3.0).abs() < 1e-9));
        assert!(p.iter().any(|v| (v.z - 2.0).abs() < 1e-9));
    }
}

#[test]
fn test_ridge_apex_propagates_into_end_face() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut().add_face(
        &[
            V3(0., 0., 2.),
            V3(4., 0., 2.),
            V3(4., 2., 2.),
            V3(0., 2., 2.),
        ],
        &[],
    );
    s.mesh_mut().add_face(
        &[
            V3(0., 0., 0.),
            V3(4., 0., 0.),
            V3(4., 0., 2.),
            V3(0., 0., 2.),
        ],
        &[],
    );
    for seg in [
        (V3(0., 0., 2.), V3(4., 0., 2.)),
        (V3(0., 2., 2.), V3(4., 2., 2.)),
    ] {
        let c = build_add_edges(&s, &[seg], false, vec![], None);
        exec(&mut s, &mut h, c);
    }
    let c = build_add_edges(&s, &[(V3(2., 0., 2.), V3(2., 2., 2.))], true, vec![], None);
    exec(&mut s, &mut h, c);
    let m = s.mesh();
    let wall = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|v| v.y.abs() < 1e-9))
        .unwrap();
    let p = m.face_positions(wall);
    assert!(p
        .iter()
        .any(|v| (v.x - 2.0).abs() < 1e-9 && v.y.abs() < 1e-9));
    assert_eq!(p.len(), 5);
    h.run(
        &mut s,
        MoveVerticesCommand {
            src: vec![V3(2., 0., 2.), V3(2., 2., 2.)],
            delta: V3(0., 0., 1.),
        },
    );
    assert!(s
        .mesh()
        .face_positions(wall)
        .iter()
        .any(|v| (v.z - 3.0).abs() < 1e-9));
}
