// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Consistent outward orientation of closed solids (core.orient).

use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::{DVec3, Mesh};

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn orient(m: &mut Mesh) -> usize {
    orient_outward(m, DEFAULT_SEED, None).len()
}

fn cube(m: &mut Mesh) {
    m.add_face(
        &[V(0., 0., 0.), V(1., 0., 0.), V(1., 1., 0.), V(0., 1., 0.)],
        &[],
    );
    m.add_face(
        &[V(0., 0., 1.), V(1., 0., 1.), V(1., 1., 1.), V(0., 1., 1.)],
        &[],
    );
    m.add_face(
        &[V(0., 0., 0.), V(1., 0., 0.), V(1., 0., 1.), V(0., 0., 1.)],
        &[],
    );
    m.add_face(
        &[V(0., 1., 0.), V(1., 1., 0.), V(1., 1., 1.), V(0., 1., 1.)],
        &[],
    );
    m.add_face(
        &[V(0., 0., 0.), V(0., 1., 0.), V(0., 1., 1.), V(0., 0., 1.)],
        &[],
    );
    m.add_face(
        &[V(1., 0., 0.), V(1., 1., 0.), V(1., 1., 1.), V(1., 0., 1.)],
        &[],
    );
}

fn prism(m: &mut Mesh, h: f64) {
    let (a, b, c) = ((-0.2, -2.8), (2.7, -7.2), (4.1, -2.7));
    let base = [V(a.0, a.1, 0.), V(b.0, b.1, 0.), V(c.0, c.1, 0.)];
    let top = [V(a.0, a.1, h), V(b.0, b.1, h), V(c.0, c.1, h)];
    m.add_face(&base, &[]);
    m.add_face(&top, &[]);
    for i in 0..3 {
        let j = (i + 1) % 3;
        m.add_face(&[base[i], base[j], top[j], top[i]], &[]);
    }
}

fn all_outward(m: &Mesh, center: DVec3) -> bool {
    m.face_ids()
        .iter()
        .all(|&f| m.face_normal(f).dot(m.face_centroid(f) - center) > 0.0)
}

#[test]
fn test_single_face_is_open() {
    let mut m = Mesh::new();
    m.add_face(
        &[V(0., 0., 0.), V(1., 0., 0.), V(1., 1., 0.), V(0., 1., 0.)],
        &[],
    );
    assert!(!is_closed(&m));
}

#[test]
fn test_two_faces_sharing_one_edge_is_open() {
    let mut m = Mesh::new();
    m.add_face(
        &[V(0., 0., 0.), V(1., 0., 0.), V(1., 1., 0.), V(0., 1., 0.)],
        &[],
    );
    m.add_face(
        &[V(1., 0., 0.), V(2., 0., 0.), V(2., 1., 0.), V(1., 1., 0.)],
        &[],
    );
    assert!(!is_closed(&m));
}

#[test]
fn test_cube_is_closed() {
    let mut m = Mesh::new();
    cube(&mut m);
    assert!(is_closed(&m));
}

#[test]
fn test_cube_oriented_all_outward() {
    let mut m = Mesh::new();
    cube(&mut m);
    orient(&mut m);
    assert!(all_outward(&m, V(0.5, 0.5, 0.5)));
}

#[test]
fn test_cube_signed_volume_positive_and_correct() {
    let mut m = Mesh::new();
    cube(&mut m);
    orient(&mut m);
    assert!((signed_volume(&m) - 1.0).abs() < 1e-6);
}

#[test]
fn test_cube_orientation_is_idempotent() {
    let mut m = Mesh::new();
    cube(&mut m);
    orient(&mut m);
    assert_eq!(orient(&mut m), 0);
}

#[test]
fn test_cube_restores_a_deliberate_flip() {
    let mut m = Mesh::new();
    cube(&mut m);
    orient(&mut m);
    let f = m.face_ids()[0];
    let mut outer = m.face_positions(f);
    outer.reverse();
    m.remove_face(f);
    m.add_face(&outer, &[]);
    assert_eq!(orient(&mut m), 1);
    assert!(all_outward(&m, V(0.5, 0.5, 0.5)));
}

#[test]
fn test_prism_oriented_all_outward() {
    let mut m = Mesh::new();
    prism(&mut m, 3.0);
    orient(&mut m);
    let cx = (-0.2 + 2.7 + 4.1) / 3.0;
    let cy = (-2.8 - 7.2 - 2.7) / 3.0;
    assert!(all_outward(&m, V(cx, cy, 1.5)));
}

#[test]
fn test_prism_signed_volume_positive() {
    let mut m = Mesh::new();
    prism(&mut m, 3.0);
    orient(&mut m);
    assert!(signed_volume(&m) > 0.0);
}

fn two_level_box(m: &mut Mesh) {
    let lo = [V(0., 0., 0.), V(1., 0., 0.), V(1., 1., 0.), V(0., 1., 0.)];
    let mid = [V(0., 0., 1.), V(1., 0., 1.), V(1., 1., 1.), V(0., 1., 1.)];
    let hi = [V(0., 0., 2.), V(1., 0., 2.), V(1., 1., 2.), V(0., 1., 2.)];
    m.add_face(&lo, &[]);
    m.add_face(&mid, &[]);
    m.add_face(&hi, &[]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        m.add_face(&[lo[i], lo[j], mid[j], mid[i]], &[]);
        m.add_face(&[mid[i], mid[j], hi[j], hi[i]], &[]);
    }
}

#[test]
fn test_interior_partition_is_not_flipped() {
    let mut m = Mesh::new();
    two_level_box(&mut m);
    let slab = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|v| (v.z - 1.0).abs() < 1e-9))
        .unwrap();
    let before = m.face_positions(slab);
    orient(&mut m);
    assert_eq!(m.face_positions(slab), before);
}

#[test]
fn test_orientation_idempotent_with_interior_partition() {
    let mut m = Mesh::new();
    two_level_box(&mut m);
    orient(&mut m);
    assert_eq!(orient(&mut m), 0);
    assert_eq!(orient(&mut m), 0);
}

#[test]
fn test_open_sheet_is_noop() {
    let mut m = Mesh::new();
    m.add_face(
        &[V(0., 0., 0.), V(1., 0., 0.), V(1., 1., 0.), V(0., 1., 0.)],
        &[],
    );
    assert_eq!(orient(&mut m), 0);
}

#[test]
fn test_flat_plan_is_noop() {
    let mut m = Mesh::new();
    m.add_face(
        &[V(0., 0., 0.), V(2., 0., 0.), V(2., 2., 0.), V(0., 2., 0.)],
        &[],
    );
    m.add_face(
        &[V(2., 0., 0.), V(4., 0., 0.), V(4., 2., 0.), V(2., 2., 0.)],
        &[],
    );
    assert_eq!(orient(&mut m), 0);
}

fn box_at(m: &mut Mesh, ox: f64, oy: f64, s: f64) {
    let p = |x: f64, y: f64, z: f64| V(ox + x * s, oy + y * s, z * s);
    m.add_face(
        &[p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
        &[],
    );
    m.add_face(
        &[p(0., 1., 0.), p(1., 1., 0.), p(1., 1., 1.), p(0., 1., 1.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(0., 1., 0.), p(0., 1., 1.), p(0., 0., 1.)],
        &[],
    );
    m.add_face(
        &[p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)],
        &[],
    );
}

#[test]
fn test_disjoint_solids_each_oriented_outward() {
    let mut m = Mesh::new();
    box_at(&mut m, 0.0, 0.0, 1.0);
    box_at(&mut m, 5.0, 0.0, 1.0);
    orient(&mut m);
    for center in [V(0.5, 0.5, 0.5), V(5.5, 0.5, 0.5)] {
        assert!(m
            .face_ids()
            .iter()
            .filter(|&&f| (m.face_centroid(f).x - center.x).abs() < 1.0)
            .all(|&f| m.face_normal(f).dot(m.face_centroid(f) - center) > 0.0));
    }
}

#[test]
fn test_disjoint_solid_is_not_an_interior_partition() {
    let mut m = Mesh::new();
    box_at(&mut m, 0.0, 0.0, 1.0);
    box_at(&mut m, 5.0, 0.0, 1.0);
    orient(&mut m);
    assert!(!m.face_ids().iter().any(|&f| m.face(f).interior));
}

#[test]
fn test_only_restricts_the_pass_to_its_components() {
    let mut m = Mesh::new();
    box_at(&mut m, 0.0, 0.0, 1.0);
    box_at(&mut m, 5.0, 0.0, 1.0);
    let far: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_centroid(f).x > 4.0)
        .collect();
    let before: Vec<_> = far.iter().map(|&f| m.face_positions(f)).collect();
    let near = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_centroid(f).x < 1.0)
        .unwrap();
    orient_outward(&mut m, DEFAULT_SEED, Some(&[near]));
    let after: Vec<_> = far.iter().map(|&f| m.face_positions(f)).collect();
    assert_eq!(after, before);
    let center = V(0.5, 0.5, 0.5);
    assert!(m
        .face_ids()
        .iter()
        .filter(|&&f| m.face_centroid(f).x < 1.0)
        .all(|&f| m.face_normal(f).dot(m.face_centroid(f) - center) > 0.0));
}

#[test]
fn test_only_matches_the_full_pass_on_the_component_it_scopes() {
    let mut m = Mesh::new();
    box_at(&mut m, 0.0, 0.0, 1.0);
    box_at(&mut m, 5.0, 0.0, 1.0);
    let mut scoped = Mesh::new();
    box_at(&mut scoped, 0.0, 0.0, 1.0);
    box_at(&mut scoped, 5.0, 0.0, 1.0);
    orient(&mut m);
    let near = scoped
        .face_ids()
        .into_iter()
        .find(|&f| scoped.face_centroid(f).x < 1.0)
        .unwrap();
    orient_outward(&mut scoped, DEFAULT_SEED, Some(&[near]));
    let full: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_centroid(f).x < 1.0)
        .map(|f| m.face_positions(f))
        .collect();
    let part: Vec<_> = scoped
        .face_ids()
        .into_iter()
        .filter(|&f| scoped.face_centroid(f).x < 1.0)
        .map(|f| scoped.face_positions(f))
        .collect();
    assert_eq!(part, full);
}

#[test]
fn test_nearby_open_sheet_does_not_mark_a_solid_face_interior() {
    let mut m = Mesh::new();
    box_at(&mut m, 0.0, 0.0, 1.0);
    m.add_face(
        &[
            V(5., -1., -1.),
            V(5., 2., -1.),
            V(5., 2., 2.),
            V(5., -1., 2.),
        ],
        &[],
    );
    orient(&mut m);
    let right = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|v| (v.x - 1.0).abs() < 1e-9))
        .unwrap();
    assert!(!m.face(right).interior);
    assert!(
        m.face_normal(right)
            .dot(m.face_centroid(right) - V(0.5, 0.5, 0.5))
            > 0.0
    );
}
