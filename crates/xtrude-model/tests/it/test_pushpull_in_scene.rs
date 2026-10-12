// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `push_face_in_scene` pushes exactly as `push_face` does, without a
//! history step of its own.

use xtrude_model::pushpull::{push_face_in_scene, PushOutcome};
use xtrude_model::scene::mesh_equal;

use crate::common::{self, V};

#[test]
fn in_scene_push_gives_the_same_mesh_as_push_face() {
    let (expected, _) = common::cube_scene(1.0, 0.5);

    let (mut s, mut h) = common::scene_history();
    common::rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)],
    );
    let f = s.mesh().face_ids()[0];
    let d = common::up(&s, f, 0.5);
    let steps = h.undo_len();
    let mid = s.active_mesh_id();
    let pp = push_face_in_scene(&mut s, mid, f, d, false)
        .expect("the push runs")
        .expect("the push happens");

    assert_eq!(pp.outcome(), PushOutcome::Committed);
    assert!(mesh_equal(s.mesh(), expected.mesh()));
    assert_eq!(h.undo_len(), steps, "no history step of its own");
}

#[test]
fn in_scene_push_of_a_missing_face_does_nothing() {
    let (mut s, _) = common::cube_scene(1.0, 0.5);
    let before = s.mesh().clone();
    let gone = s.mesh().face_ids()[0];
    s.mesh_mut().remove_face(gone);
    let after_removal = s.mesh().clone();
    let mid = s.active_mesh_id();
    let pp = push_face_in_scene(&mut s, mid, gone, 0.2, false).expect("no error");
    assert!(pp.is_none());
    assert!(mesh_equal(s.mesh(), &after_removal));
    assert!(!mesh_equal(s.mesh(), &before));
}

#[test]
fn in_scene_push_too_short_does_nothing() {
    let (mut s, _) = common::cube_scene(1.0, 0.5);
    let before = s.mesh().clone();
    let top =
        common::find_face(&s, |p| p.iter().all(|q| (q.z - 0.5).abs() < 1e-9)).expect("top face");
    let mid = s.active_mesh_id();
    let pp = push_face_in_scene(&mut s, mid, top, 1e-7, false).expect("no error");
    assert!(pp.is_none());
    assert!(mesh_equal(s.mesh(), &before));
}
