// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Pulling a face up against a wall.

use crate::common;
use common::*;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_model::edits::build_add_edges;
use xtrude_model::pushpull::PushOutcome;

fn step_with_square() -> (xtrude_model::Scene, xtrude_core::FaceId) {
    let (mut s, mut h) = scene_history();
    ux_cube(&mut s, &mut h, 4.0, 3.0);
    let c = build_add_edges(&s, &[(V3(0., 2., 3.), V3(4., 2., 3.))], true, vec![], None);
    exec(&mut s, &mut h, c);
    let front = faces_where(&s, |v| (v.z - 3.0).abs() < 1e-9)
        .into_iter()
        .find(|&f| {
            s.mesh()
                .face_positions(f)
                .iter()
                .map(|v| v.y)
                .fold(f64::MIN, f64::max)
                <= 2.0 + 1e-9
        })
        .unwrap();
    push_ux(&mut s, front, -1.0, false);
    let c = build_add_edges(
        &s,
        &[
            (V3(1., 1., 2.), V3(2., 1., 2.)),
            (V3(2., 1., 2.), V3(2., 2., 2.)),
            (V3(1., 1., 2.), V3(1., 2., 2.)),
        ],
        true,
        vec![],
        None,
    );
    exec(&mut s, &mut h, c);
    let square = faces_where(&s, |v| (v.z - 2.0).abs() < 1e-9)
        .into_iter()
        .find(|&f| {
            s.mesh().face_positions(f).len() == 4 && (s.mesh().face_area(f) - 1.0).abs() < 1e-6
        })
        .unwrap();
    assert!((signed_volume(s.mesh()) - 40.0).abs() < 1e-6);
    (s, square)
}

#[test]
fn test_pulling_against_the_riser_keeps_a_true_solid() {
    for d in [0.5, 1.0, 1.5, -0.5] {
        let (mut s, square) = step_with_square();
        let (_, out) = push_ux(&mut s, square, d, false);
        let m = s.mesh();
        assert_eq!(out, PushOutcome::Committed, "d={d}");
        assert!(is_closed(m), "d={d}");
        assert!(!m.face_ids().iter().any(|&f| m.face(f).interior), "d={d}");
        assert!(m.edges.iter().all(|(_, e)| e.faces.len() == 2), "d={d}");
        assert!((signed_volume(m) - (40.0 + d)).abs() < 1e-6, "d={d}");
    }
}

#[test]
fn test_the_riser_loses_the_strip_the_pull_covers() {
    let (mut s, square) = step_with_square();
    push_ux(&mut s, square, 0.5, false);
    let riser = faces_where(&s, |v| (v.y - 2.0).abs() < 1e-9);
    assert_eq!(riser.len(), 1);
    assert!((s.mesh().face_area(riser[0]) - (4.0 * 1.0 - 1.0 * 0.5)).abs() < 1e-6);
}
