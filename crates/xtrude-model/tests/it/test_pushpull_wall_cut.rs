// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `PushPull::wall_cut`: the strips a push into the solid takes off the
//! walls it slides down, and what stays of them.

use crate::common;
use common::*;
use xtrude_core::geometry::PolyFace;
use xtrude_core::math::DVec3;
use xtrude_model::edits::build_add_edges;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::PushPull;
use xtrude_model::Scene;

/// A 4 × 4 × 2 box whose top is split along y = 2; returns the scene and
/// the front half of the top (y ≤ 2).
fn split_top() -> (Scene, xtrude_core::FaceId) {
    let (mut s, mut h) = cube_scene(4.0, 2.0);
    let c = build_add_edges(&s, &[(V3(0., 2., 2.), V3(4., 2., 2.))], true, vec![], None);
    exec(&mut s, &mut h, c);
    let half = find_face(&s, |p| {
        all_near(p, |v| v.z, 2.0) && p.iter().all(|v| v.y <= 2.0 + 1e-9)
    })
    .expect("front half of the top");
    (s, half)
}

fn drag(s: &mut Scene, face: xtrude_core::FaceId, d: f64) -> PushPull {
    let mut pp = PushPull::new();
    pp.begin(s, MeshId::LOOSE, face);
    assert!(pp.normal.unwrap().z > 0.99);
    pp.extrusion = d;
    pp
}

fn area(p: &PolyFace) -> f64 {
    let newell = |lp: &[DVec3]| {
        (0..lp.len())
            .map(|i| lp[i].cross(lp[(i + 1) % lp.len()]))
            .sum::<DVec3>()
            .length()
            / 2.0
    };
    newell(&p.vertices) - p.holes.iter().map(|h| newell(h)).sum::<f64>()
}

#[test]
fn test_a_push_in_cuts_the_walls_it_slides_down() {
    let (mut s, half) = split_top();
    let pp = drag(&mut s, half, -0.5);
    let m = s.mesh();
    let cut = pp.wall_cut(m);
    // The front wall and both side walls; not the back wall.
    assert_eq!(cut.walls.len(), 3);
    assert_eq!(cut.sides.len(), 3);
    assert!(cut.walls.iter().all(|&w| {
        let pts = m.face_positions(w);
        all_near(&pts, |v| v.y, 0.0) || all_near(&pts, |v| v.x, 0.0) || all_near(&pts, |v| v.x, 4.0)
    }));
    // One piece stays of each wall.
    assert_eq!(cut.remains.len(), 3);
    for rest in &cut.remains {
        if all_near(&rest.vertices, |v| v.y, 0.0) {
            // The whole front wall comes down to z = 1.5.
            assert!((area(rest) - 4.0 * 1.5).abs() < 1e-9);
            assert!(rest.vertices.iter().all(|v| v.z <= 1.5 + 1e-9));
        } else {
            // A side wall loses its front half's top strip only.
            assert!((area(rest) - (4.0 * 2.0 - 2.0 * 0.5)).abs() < 1e-9);
        }
    }
}

#[test]
fn test_a_recess_in_the_middle_of_a_face_cuts_no_wall() {
    let (mut s, mut h) = cube_scene(4.0, 2.0);
    rectangle(
        &mut s,
        &mut h,
        &[
            V3(1., 1., 2.),
            V3(3., 1., 2.),
            V3(3., 3., 2.),
            V3(1., 3., 2.),
        ],
    );
    let inner = find_face(&s, |p| {
        all_near(p, |v| v.z, 2.0) && p.iter().all(|v| (1.0..=3.0).contains(&v.x))
    })
    .expect("the drawn rectangle");
    let pp = drag(&mut s, inner, -0.5);
    assert_eq!(pp.wall_cut(s.mesh()), Default::default());
}

#[test]
fn test_a_pull_or_a_kept_base_cuts_nothing() {
    let (mut s, half) = split_top();
    let pp = drag(&mut s, half, 0.5);
    assert!(pp.wall_cut(s.mesh()).walls.is_empty());
    let mut pp = drag(&mut s, half, -0.5);
    pp.keep_base = true;
    assert!(pp.wall_cut(s.mesh()).walls.is_empty());
}

#[test]
fn test_risers_between_two_cut_sides_are_left_out() {
    let (mut s, half) = split_top();
    let pp = drag(&mut s, half, -0.5);
    let m = s.mesh();
    let (_faces, rings) = pp.light_faces(m);
    let cut = pp.wall_cut(m);
    let all = PushPull::rubber_band_lines(&rings);
    let kept = PushPull::rubber_band_lines_cut(&rings, &cut.sides);
    assert_eq!(all.len(), 8);
    assert_eq!(kept.len(), 6);
    // The front corners lose their risers; the ones on the split line stay.
    let riser_at = |segs: &[(DVec3, DVec3)], x: f64, y: f64| {
        segs.iter()
            .any(|(a, b)| a.x == x && b.x == x && a.y == y && b.y == y && a.z != b.z)
    };
    assert!(!riser_at(&kept, 0.0, 0.0) && !riser_at(&kept, 4.0, 0.0));
    assert!(riser_at(&kept, 0.0, 2.0) && riser_at(&kept, 4.0, 2.0));
}

#[test]
fn test_an_open_shell_cuts_nothing() {
    let (mut s, half) = split_top();
    let bottom = find_face(&s, |p| all_near(p, |v| v.z, 0.0)).expect("the bottom");
    s.mesh_mut().remove_face(bottom);
    let pp = drag(&mut s, half, -0.5);
    assert!(pp.wall_cut(s.mesh()).walls.is_empty());
}
