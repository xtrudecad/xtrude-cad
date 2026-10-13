// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Revolving a face about an axis in its plane: a full turn along the
//! sweep's lathe path, a partial one along an arc, each a closed solid.

use std::f64::consts::PI;

use xtrude_core::math::VecExt;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::sweep::{
    revolve_preview_faces, revolve_profile, revolve_spans, RevolveRefusal, REVOLVE_SIDES,
};
use xtrude_core::{DVec3, FaceId, Mesh};

const MM: f64 = 0.001;

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z) * MM
}

/// A rectangle in the XZ plane, `x0..x1` by `0..h` (mm).
fn profile(x0: f64, x1: f64, h: f64) -> (Mesh, FaceId) {
    let mut m = Mesh::new();
    let f = m.add_face(
        &[p(x0, 0., 0.), p(x1, 0., 0.), p(x1, 0., h), p(x0, 0., h)],
        &[],
    );
    (m, f)
}

fn mm3(m: &Mesh) -> f64 {
    signed_volume(m) / (MM * MM * MM)
}

/// The volume of `spans` spans of `step_deg` each, turning a rectangle
/// `r0..r1` by `h` (mm) whose corners lie on the stations.
fn polygon_volume(r0: f64, r1: f64, h: f64, spans: usize, step_deg: f64) -> f64 {
    h * spans as f64 * 0.5 * step_deg.to_radians().sin() * (r1 * r1 - r0 * r0)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs()
}

#[test]
fn test_a_full_turn_of_a_rectangle_off_the_axis_is_a_closed_tube() {
    // 10 × 20 mm, 5 mm off the Z axis
    let (mut m, f) = profile(5.0, 15.0, 20.0);
    revolve_profile(&mut m, f, DVec3::ZERO, DVec3::Z, 360.0, REVOLVE_SIDES).unwrap();
    assert!(is_closed(&m));
    let v = mm3(&m);
    let want = polygon_volume(5.0, 15.0, 20.0, 24, 15.0);
    assert!(close(v, want), "{v} vs {want}");
    // Pappus: area × the centroid's path; the 24-gon falls short by 1.1 %
    let round = 200.0 * 2.0 * PI * 10.0;
    assert!(v < round && (round - v) / round < 0.012, "{v} vs {round}");
    // the 24 outer walls stand 15 mm from the axis at their corners
    let r_max = m
        .vertex_ids()
        .into_iter()
        .map(|id| m.pos(id).truncate().length() / MM)
        .fold(0.0, f64::max);
    assert!((r_max - 15.0).abs() < 1e-9);
}

#[test]
fn test_a_quarter_turn_is_closed_with_planar_end_faces() {
    let (mut m, f) = profile(5.0, 15.0, 20.0);
    revolve_profile(&mut m, f, DVec3::ZERO, DVec3::Z, 90.0, REVOLVE_SIDES).unwrap();
    assert!(is_closed(&m));
    assert_eq!(revolve_spans(90.0, REVOLVE_SIDES), 6);
    let v = mm3(&m);
    let want = polygon_volume(5.0, 15.0, 20.0, 6, 15.0);
    assert!(close(v, want), "{v} vs {want}");
    // one end face in the XZ plane, where the profile was; one in YZ
    for (on_plane, normal) in [
        (
            Box::new(|q: DVec3| q.y.abs() < 1e-12) as Box<dyn Fn(DVec3) -> bool>,
            DVec3::Y,
        ),
        (Box::new(|q: DVec3| q.x.abs() < 1e-12), DVec3::X),
    ] {
        let ends: Vec<FaceId> = m
            .face_ids()
            .into_iter()
            .filter(|&f| m.face_positions(f).into_iter().all(&on_plane))
            .collect();
        assert_eq!(ends.len(), 1);
        let e = ends[0];
        assert_eq!(m.face_positions(e).len(), 4);
        assert!(m.face_normal(e).unit_or_zero().dot(normal).abs() > 1.0 - 1e-12);
        assert!(close(m.face_area(e) / (MM * MM), 200.0));
    }
}

#[test]
fn test_the_turn_goes_either_way() {
    let (mut m, f) = profile(5.0, 15.0, 20.0);
    revolve_profile(&mut m, f, DVec3::ZERO, DVec3::Z, -90.0, REVOLVE_SIDES).unwrap();
    assert!(is_closed(&m));
    // turned the other way: into −Y
    assert!(m.vertex_ids().into_iter().all(|id| m.pos(id).y <= 1e-12));
    assert!(close(mm3(&m), polygon_volume(5.0, 15.0, 20.0, 6, 15.0)));
}

#[test]
fn test_a_profile_touching_the_axis_turns_into_a_solid_cylinder() {
    let (mut m, f) = profile(0.0, 10.0, 20.0);
    revolve_profile(&mut m, f, DVec3::ZERO, DVec3::Z, 360.0, 12).unwrap();
    assert!(is_closed(&m));
    assert!(close(mm3(&m), polygon_volume(0.0, 10.0, 20.0, 12, 30.0)));
}

#[test]
fn test_the_side_count_sets_the_spans() {
    assert_eq!(revolve_spans(360.0, 24), 24);
    assert_eq!(revolve_spans(360.0, 48), 48);
    assert_eq!(revolve_spans(100.0, 24), 7);
    assert_eq!(revolve_spans(1.0, 24), 1);
    assert_eq!(revolve_spans(360.0, 1), 3);
    let (mut m, f) = profile(5.0, 15.0, 20.0);
    revolve_profile(&mut m, f, DVec3::ZERO, DVec3::Z, 360.0, 48).unwrap();
    assert!(close(mm3(&m), polygon_volume(5.0, 15.0, 20.0, 48, 7.5)));
}

#[test]
fn test_refusals_leave_the_face_alone() {
    let cases = [
        // the axis through the middle of the face
        (
            p(10.0, 0.0, 0.0),
            DVec3::Z,
            360.0,
            RevolveRefusal::CrossesFace,
        ),
        // square to the face's plane
        (DVec3::ZERO, DVec3::Y, 360.0, RevolveRefusal::OffPlane),
        // parallel to the plane, but off it
        (p(0.0, 1.0, 0.0), DVec3::Z, 360.0, RevolveRefusal::OffPlane),
        (DVec3::ZERO, DVec3::Z, 0.0, RevolveRefusal::Angle),
        (DVec3::ZERO, DVec3::Z, 400.0, RevolveRefusal::Angle),
    ];
    for (origin, axis, deg, why) in cases {
        let (mut m, f) = profile(5.0, 15.0, 20.0);
        let before = m.face_positions(f);
        assert_eq!(revolve_profile(&mut m, f, origin, axis, deg, 24), Err(why));
        assert_eq!(m.face_ids(), vec![f]);
        assert_eq!(m.face_positions(f), before);
    }
}

#[test]
fn test_the_preview_shows_the_walls_and_the_far_end() {
    let (m, f) = profile(5.0, 15.0, 20.0);
    let pf = m.poly_face(f);
    // 4 walls per span; the partial turn adds its far end
    assert_eq!(
        revolve_preview_faces(&pf, DVec3::ZERO, DVec3::Z, 360.0, 24).len(),
        96
    );
    let quarter = revolve_preview_faces(&pf, DVec3::ZERO, DVec3::Z, 90.0, 24);
    assert_eq!(quarter.len(), 25);
    let end = &quarter[24];
    assert!(end.vertices.iter().all(|q| q.x.abs() < 1e-12));
    // a refused revolve previews nothing
    let through = p(10.0, 0.0, 0.0);
    assert!(revolve_preview_faces(&pf, through, DVec3::Z, 360.0, 24).is_empty());
}
