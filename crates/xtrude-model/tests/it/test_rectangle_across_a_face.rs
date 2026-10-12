// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A rectangle drawn ACROSS a face splits it.

use crate::common;
use common::*;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_model::edits::build_add_edges;
use xtrude_model::pushpull::PushOutcome;

fn rect(
    s: &mut xtrude_model::Scene,
    h: &mut xtrude_model::History,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    z: f64,
) {
    rectangle(
        s,
        h,
        &[V3(x0, y0, z), V3(x1, y0, z), V3(x1, y1, z), V3(x0, y1, z)],
    );
}

fn step() -> (xtrude_model::Scene, xtrude_model::History) {
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
    (s, h)
}

fn areas_at(s: &xtrude_model::Scene, z: f64) -> Vec<f64> {
    let mut a: Vec<f64> = faces_where(s, |v| (v.z - z).abs() < 1e-9)
        .into_iter()
        .map(|f| (s.mesh().face_area(f) * 1e6).round() / 1e6)
        .collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    a
}

#[test]
fn test_the_band_splits_the_tread() {
    for (x0, x1, pieces) in [
        (1., 2., vec![2.0, 2.0, 4.0]),
        (0., 1., vec![2.0, 6.0]),
        (3., 4., vec![2.0, 6.0]),
    ] {
        let (mut s, mut h) = step();
        rect(&mut s, &mut h, x0, x1, 0., 2., 2.);
        assert_eq!(areas_at(&s, 2.0), pieces, "x0={x0}");
        assert!(
            s.mesh().edges.iter().all(|(_, e)| e.faces.len() == 2),
            "x0={x0}"
        );
        h.undo(&mut s);
        assert_eq!(areas_at(&s, 2.0), vec![8.0]);
    }
}

#[test]
fn test_pulling_the_band_keeps_a_true_solid() {
    for d in [0.5, -0.5] {
        let (mut s, mut h) = step();
        rect(&mut s, &mut h, 0., 1., 0., 2., 2.);
        let band = faces_where(&s, |v| (v.z - 2.0).abs() < 1e-9)
            .into_iter()
            .find(|&f| {
                (s.mesh().face_area(f) - 2.0).abs() < 1e-9
                    && s.mesh()
                        .face_positions(f)
                        .iter()
                        .map(|v| v.x)
                        .fold(f64::MIN, f64::max)
                        <= 1.0 + 1e-9
            })
            .unwrap();
        let (_, out) = push_ux(&mut s, band, d, false);
        assert_eq!(out, PushOutcome::Committed);
        assert!(is_closed(s.mesh()));
        assert!(s.mesh().edges.iter().all(|(_, e)| e.faces.len() == 2));
        assert!((signed_volume(s.mesh()) - (40.0 + 2.0 * d)).abs() < 1e-6);
    }
}

#[test]
fn test_a_band_across_a_flat_face_on_the_ground() {
    let (mut s, mut h) = scene_history();
    rect(&mut s, &mut h, 0., 4., 0., 3., 0.);
    rect(&mut s, &mut h, 1., 2., 0., 3., 0.);
    assert_eq!(areas_at(&s, 0.0), vec![3.0, 3.0, 6.0]);
}
