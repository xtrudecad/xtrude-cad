// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An Offset on a CONCAVE face keeps both halves — the heal pass judges
//! "inside" by a true interior point, not the vertex average (which a U
//! swallows).

use crate::common;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::offset::OffsetTool;
use xtrude_core::topology::{
    heal_overlapping_faces, interior_point, loop_inside_loop, point_inside_2d,
};
use xtrude_core::triangulate::plane_axes;
use xtrude_core::Mesh;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// A U: the notch between the arms swallows the vertex average.
fn u() -> Vec<DVec3> {
    vec![
        v(0., 0.),
        v(6., 0.),
        v(6., 5.),
        v(4., 5.),
        v(4., 2.),
        v(2., 2.),
        v(2., 5.),
        v(0., 5.),
    ]
}

fn average(lp: &[DVec3]) -> DVec3 {
    lp.iter().copied().sum::<DVec3>() / lp.len() as f64
}

fn inside(point: DVec3, lp: &[DVec3], normal: DVec3) -> bool {
    let (a, b) = plane_axes(normal);
    let o = lp[0];
    let proj = |p: DVec3| ((p - o).dot(a), (p - o).dot(b));
    let poly: Vec<(f64, f64)> = lp.iter().map(|&p| proj(p)).collect();
    point_inside_2d(proj(point), &poly)
}

#[test]
fn test_the_vertex_average_of_a_concave_loop_lands_outside_it() {
    assert!(!inside(average(&u()), &u(), DVec3::Z));
}

#[test]
fn test_an_interior_point_really_is_inside() {
    let p = interior_point(&u(), DVec3::Z).expect("a point");
    assert!(inside(p, &u(), DVec3::Z));
}

#[test]
fn test_a_concave_loop_is_inside_itself() {
    assert!(loop_inside_loop(&u(), &u(), DVec3::Z));
}

#[test]
fn test_heal_keeps_a_concave_ring_and_the_face_filling_its_hole() {
    let inner: Vec<DVec3> = u()
        .iter()
        .map(|p| v(p.x * 0.9 + 0.3, p.y * 0.9 + 0.25))
        .collect();
    let mut mesh = Mesh::new();
    let ring = mesh.add_face(&u(), std::slice::from_ref(&inner));
    let filling = mesh.add_face(&inner, &[]);
    heal_overlapping_faces(&mut mesh, 0.5, None, None);
    assert!(mesh.has_face(ring), "the ring was healed away");
    assert!(mesh.has_face(filling));
}

#[test]
fn test_offset_on_a_concave_face_keeps_both_halves() {
    let mut vp = stub();
    let face = vp.scene.mesh_mut().add_face(&u(), &[]);
    vp.scene.mesh_mut().face_mut(face).attrs.paint.mat = Some("Slate grey".into());
    let mut t = OffsetTool::new();
    let m = vp.scene.mesh();
    t.base_face = Some((MeshId::LOOSE, face));
    t.lp = m.face_positions(face);
    t.normal = Some(m.face_normal(face));
    t.distance = 0.10;
    t.commit_now(&mut vp);
    let m = vp.scene.mesh();
    let ids = m.face_ids();
    assert_eq!(ids.len(), 2, "expected ring + inner");
    assert_eq!(
        ids.iter().filter(|&&f| !m.face(f).holes.is_empty()).count(),
        1,
        "no ring left"
    );
    for f in ids {
        assert_eq!(m.face(f).attrs.paint.mat.as_deref(), Some("Slate grey"));
    }
}
