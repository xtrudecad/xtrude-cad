// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `PushPull::accepts` says whether a commit would push exactly the
//! distance asked, by trying the push on a copy: the mesh stays as it is.

use xtrude_core::Mesh;
use xtrude_model::pushpull::PushPull;
use xtrude_model::scene::mesh_equal;

use crate::common;

fn break_it(m: &mut Mesh) {
    let f = m.face_ids()[0];
    m.remove_face(f);
}

fn on_the_top() -> (xtrude_model::scene::Scene, PushPull) {
    let (s, _) = common::cube_scene(1.0, 0.5);
    let top =
        common::find_face(&s, |p| p.iter().all(|q| (q.z - 0.5).abs() < 1e-9)).expect("top face");
    let mut pp = PushPull::new();
    pp.setup_plain(s.mesh(), s.active_mesh_id(), top);
    (s, pp)
}

#[test]
fn test_a_push_that_commits_is_accepted_and_the_mesh_is_untouched() {
    let (s, mut pp) = on_the_top();
    let before = s.mesh().clone();
    for d in [0.3, -0.2] {
        pp.extrusion = d;
        assert!(pp.accepts(s.mesh()), "{d}");
    }
    assert!(mesh_equal(s.mesh(), &before));
    assert!(!pp.refused);
}

#[test]
fn test_a_push_that_breaks_the_solid_is_not_accepted() {
    let (s, mut pp) = on_the_top();
    pp.extrusion = 0.3;
    pp.inner_override = Some(break_it);
    assert!(!pp.accepts(s.mesh()));
}

#[test]
fn test_no_distance_is_not_accepted() {
    let (s, mut pp) = on_the_top();
    pp.extrusion = 0.0;
    assert!(!pp.accepts(s.mesh()));
}
