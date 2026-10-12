// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An offset that drops what collapses, and splits when the shape does.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::offset::OffsetTool;
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::offset::{inside, interior_point, offset_regions};
use xtrude_core::topology::offset_loop;
use xtrude_core::Mesh;
use xtrude_model::ids::MeshId;

const N: DVec3 = DVec3::Z;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn area(lp: &[DVec3]) -> f64 {
    let mut m = Mesh::new();
    let f = m.add_face(lp, &[]);
    m.face_area(f)
}

fn perimeter(lp: &[DVec3]) -> f64 {
    let n = lp.len();
    (0..n).map(|i| (lp[(i + 1) % n] - lp[i]).length()).sum()
}

fn saw() -> Vec<DVec3> {
    let mut s = vec![v(0., 0.), v(10., 0.), v(10., 5.)];
    s.extend((1..19).map(|k| {
        v(
            10.0 - 0.5 * k as f64,
            5.0 - if k % 2 == 1 { 0.3 } else { 0.0 },
        )
    }));
    s.push(v(0., 5.));
    s
}

fn dumbbell() -> Vec<DVec3> {
    vec![
        v(0., 0.),
        v(4., 0.),
        v(4., 1.7),
        v(6., 1.7),
        v(6., 0.),
        v(10., 0.),
        v(10., 4.),
        v(6., 4.),
        v(6., 2.3),
        v(4., 2.3),
        v(4., 4.),
        v(0., 4.),
    ]
}

fn rim() -> Vec<DVec3> {
    vec![v(0., 0.), v(2.4, 0.), v(2.4, 0.2), v(0., 0.2)]
}

#[test]
fn test_a_simple_offset_matches_the_slide_and_intersect_area() {
    let sq = vec![v(0., 0.), v(10., 0.), v(10., 5.), v(0., 5.)];
    assert!(offset_loop(&sq, N, 0.10).is_some());
    let r = offset_regions(&sq, N, 0.10);
    assert_eq!(r.len(), 1);
    let got = area(&r[0].0);
    assert!(area(&sq) - perimeter(&sq) * 0.10 <= got && got <= area(&sq));
}

#[test]
fn test_a_concave_loop_offsets_too() {
    let ell = vec![
        v(0., 0.),
        v(10., 0.),
        v(10., 4.),
        v(6., 4.),
        v(6., 8.),
        v(0., 8.),
    ];
    let r = offset_regions(&ell, N, 0.10);
    assert_eq!(r.len(), 1);
    assert!(area(&r[0].0) < area(&ell));
}

#[test]
fn test_a_saw_toothed_edge_no_longer_refuses() {
    assert!(!offset_regions(&saw(), N, 0.10).is_empty());
    let deep = offset_regions(&saw(), N, 0.20);
    assert!(
        !deep.is_empty(),
        "the whole face was thrown away with its pockets"
    );
    assert!(area(&deep[0].0) < area(&saw()));
}

#[test]
fn test_a_pinched_shape_comes_apart() {
    assert_eq!(offset_regions(&dumbbell(), N, 0.25).len(), 1);
    let halves = offset_regions(&dumbbell(), N, 0.40);
    assert_eq!(halves.len(), 2);
    for (lp, _h) in halves {
        assert!((area(&lp) - 3.2 * 3.2).abs() < 0.05);
    }
}

#[test]
fn test_an_offset_that_consumes_everything_returns_nothing() {
    assert!(offset_regions(&rim(), N, 0.10).is_empty());
    assert!(offset_regions(&rim(), N, 0.50).is_empty());
}

#[test]
fn test_outward_offset_grows_the_shape() {
    let sq = vec![v(0., 0.), v(10., 0.), v(10., 5.), v(0., 5.)];
    let r = offset_regions(&sq, N, -0.10);
    assert_eq!(r.len(), 1);
    assert!(area(&r[0].0) > area(&sq));
}

#[test]
fn test_degenerate_input_is_refused_quietly() {
    assert!(offset_regions(&[v(0., 0.), v(1., 0.)], N, 0.1).is_empty());
    assert!(offset_regions(&[v(0., 0.), v(1., 0.), v(2., 0.)], N, 0.1).is_empty());
    assert!(offset_regions(&dumbbell(), DVec3::ZERO, 0.1).is_empty());
}

#[test]
fn test_interior_point_of_a_concave_loop_is_really_inside() {
    let ell = vec![
        v(0., 0.),
        v(10., 0.),
        v(10., 4.),
        v(6., 4.),
        v(6., 8.),
        v(0., 8.),
    ];
    let p = interior_point(&ell, N).expect("a point");
    assert!(inside(p, &ell, N));
}

fn offset(vp: &mut Viewport, face: xtrude_core::FaceId, distance: f64) {
    let mut t = OffsetTool::new();
    let m = vp.scene.mesh();
    t.base_face = Some((MeshId::LOOSE, face));
    t.lp = m.face_positions(face);
    t.normal = Some(m.face_normal(face));
    t.distance = distance;
    t.commit_now(vp);
}

fn said(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn test_the_tool_builds_a_ring_with_one_hole_per_piece() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(&dumbbell(), &[]);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.mat = Some("Slate grey".into());
    vp.take_events();
    offset(&mut vp, f, 0.40);
    assert!(said(&mut vp).is_empty());
    let m = vp.scene.mesh();
    let rings: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| !m.face(f).holes.is_empty())
        .collect();
    assert_eq!(rings.len(), 1);
    assert_eq!(
        m.face(rings[0]).holes.len(),
        2,
        "the two halves must both be holes"
    );
    assert_eq!(m.faces.len(), 3);
    for f in m.face_ids() {
        assert_eq!(m.face(f).attrs.paint.mat.as_deref(), Some("Slate grey"));
    }
}

#[test]
fn test_the_tool_undoes_a_split_offset_in_one_step() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(&dumbbell(), &[]);
    offset(&mut vp, f, 0.40);
    assert_eq!(vp.scene.mesh().faces.len(), 3);
    vp.undo();
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_the_tool_still_refuses_out_loud_when_nothing_survives() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(&rim(), &[]);
    vp.take_events();
    offset(&mut vp, f, 0.10);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(!said(&mut vp).is_empty());
}

fn irregular_outline() -> Vec<DVec3> {
    [
        (0.0000, 0.0000),
        (0.0000, 2.4000),
        (3.8167, 2.4000),
        (3.6874, 2.1498),
        (3.5576, 1.7338),
        (3.5021, 1.3015),
        (3.5224, 0.8662),
        (3.6180, 0.4411),
        (3.7860, 0.0390),
        (3.8109, 0.0000),
    ]
    .iter()
    .map(|&(x, y)| v(x, y))
    .collect()
}

#[test]
fn test_the_face_from_the_report_takes_the_offset_it_was_asked_for() {
    let n = v3(0., 0., -1.);
    let lp = irregular_outline();
    assert!(
        offset_loop(&lp, n, 0.10).is_none(),
        "the old path is supposed to refuse this one"
    );
    for d in [0.05, 0.10, 0.20, 0.30] {
        let r = offset_regions(&lp, n, d);
        assert_eq!(r.len(), 1, "{d}");
        let got = area(&r[0].0);
        assert!(got < area(&lp) && got > 0.5 * area(&lp));
    }
}
