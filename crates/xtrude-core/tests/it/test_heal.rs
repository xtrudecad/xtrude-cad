// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Drop a redundant 'mother' face left overlapping its own subdivisions.

use xtrude_core::topology::{
    faces_coplanar, heal_overlapping_faces, point_in_face_solid, HEAL_FACE_CAP,
};
use xtrude_core::{DVec3, Mesh};

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn heal(m: &mut Mesh) -> usize {
    heal_overlapping_faces(m, 0.5, None, None).len()
}

#[test]
fn test_heal_removes_mother_covered_by_inset_subdivisions() {
    let mut m = Mesh::new();
    m.add_face(&[V(0., 0.), V(8., 0.), V(8., 8.), V(0., 8.)], &[]);
    m.add_face(&[V(0.2, 0.2), V(3.8, 0.2), V(3.8, 7.8), V(0.2, 7.8)], &[]);
    m.add_face(&[V(4.2, 0.2), V(7.8, 0.2), V(7.8, 7.8), V(4.2, 7.8)], &[]);
    assert_eq!(heal(&mut m), 1);
    assert_eq!(m.faces.len(), 2);
    assert!(m.face_ids().iter().all(|&f| m.face_area(f) < 30.0));
}

#[test]
fn test_heal_keeps_a_face_with_a_small_island_inside() {
    let mut m = Mesh::new();
    m.add_face(&[V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)], &[]);
    m.add_face(&[V(4., 4.), V(6., 4.), V(6., 6.), V(4., 6.)], &[]);
    let removed = heal_overlapping_faces(&mut m, 0.5, Some(false), None);
    assert!(removed.is_empty());
    assert_eq!(m.faces.len(), 2);
}

#[test]
fn test_heal_no_op_on_clean_subdivision() {
    let mut m = Mesh::new();
    m.add_face(&[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)], &[]);
    m.add_face(&[V(4., 0.), V(8., 0.), V(8., 4.), V(4., 4.)], &[]);
    assert_eq!(heal(&mut m), 0);
}

#[test]
fn test_heal_dedupes_nested_holes_keeps_the_ring() {
    let mut m = Mesh::new();
    let outer = vec![V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)];
    let big_hole = vec![V(2., 2.), V(8., 2.), V(8., 8.), V(2., 8.)];
    let nested = vec![V(2., 2.), V(8., 2.), V(8., 5.), V(2., 5.)];
    // add_face drops the nested hole itself; build the ring via add_hole to
    // reproduce a face that accumulated both.
    let ring = m.add_face(&outer, std::slice::from_ref(&big_hole));
    m.add_hole(ring, &nested);
    m.add_face(&big_hole, &[]);
    assert_eq!(heal(&mut m), 0);
    let ring = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .unwrap();
    assert_eq!(m.face(ring).holes.len(), 1);
}

#[test]
fn test_heal_keeps_a_legit_ring_with_an_inner_face() {
    let mut m = Mesh::new();
    m.add_face(&[V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)], &[]);
    m.add_face(
        &[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)],
        &[vec![V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)]],
    );
    assert_eq!(heal(&mut m), 0);
    assert!(m.face_ids().iter().any(|&f| !m.face(f).holes.is_empty()));
}

#[test]
fn test_heal_flips_a_reversed_face() {
    let mut m = Mesh::new();
    m.add_face(&[V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)], &[]);
    m.add_face(&[V(2., 2.), V(2., 4.), V(4., 4.), V(4., 2.)], &[]);
    let mut before: Vec<f64> = m
        .face_ids()
        .iter()
        .map(|&f| (m.face_normal(f).z * 10.0).round() / 10.0)
        .collect();
    before.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(before, vec![-1.0, 1.0]);
    heal(&mut m);
    assert!(m.face_ids().iter().all(|&f| m.face_normal(f).z > 0.0));
}

fn solid_overlap(m: &Mesh) -> usize {
    let fs = m.face_ids();
    let mut n = 0;
    for i in 0..fs.len() {
        for j in i + 1..fs.len() {
            let (a, b) = (fs[i], fs[j]);
            if !faces_coplanar(m.face_normal(a), m.face_normal(b)) {
                continue;
            }
            if m.face_area(a) > m.face_area(b)
                && point_in_face_solid(&m.poly_face(a), m.face_centroid(b))
            {
                n += 1;
            }
            if m.face_area(b) > m.face_area(a)
                && point_in_face_solid(&m.poly_face(b), m.face_centroid(a))
            {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn test_partial_overlap_resolved_by_holing_on_flat_off_in_3d() {
    let mut flat = Mesh::new();
    flat.add_face(&[V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)], &[]);
    flat.add_face(&[V(2., 2.), V(4., 2.), V(4., 4.), V(2., 4.)], &[]);
    assert_eq!(solid_overlap(&flat), 1);
    heal(&mut flat);
    assert_eq!(flat.faces.len(), 2);
    assert_eq!(solid_overlap(&flat), 0);
    assert!(flat
        .face_ids()
        .iter()
        .any(|&f| !flat.face(f).holes.is_empty()));
    assert!(flat
        .face_ids()
        .iter()
        .any(|&f| flat.face(f).holes.is_empty() && (flat.face_area(f) - 4.0).abs() < 0.1));

    let mut d3 = Mesh::new();
    d3.add_face(&[V(0., 0.), V(10., 0.), V(10., 10.), V(0., 10.)], &[]);
    d3.add_face(&[V(2., 2.), V(4., 2.), V(4., 4.), V(2., 4.)], &[]);
    d3.add_face(
        &[
            DVec3::new(0., 0., 3.),
            DVec3::new(1., 0., 3.),
            DVec3::new(1., 1., 3.),
            DVec3::new(0., 1., 3.),
        ],
        &[],
    );
    heal(&mut d3);
    assert!(!d3.face_ids().iter().any(|&f| !d3.face(f).holes.is_empty()));
}

#[test]
fn test_heal_skips_import_scale_meshes() {
    let mut m = Mesh::new();
    for i in 0..=HEAL_FACE_CAP {
        let x = (i % 100) as f64 * 2.0;
        let y = (i / 100) as f64 * 2.0;
        m.add_face(
            &[V(x, y), V(x + 1., y), V(x + 1., y + 1.), V(x, y + 1.)],
            &[],
        );
    }
    let t0 = std::time::Instant::now();
    assert_eq!(heal(&mut m), 0);
    assert!(t0.elapsed().as_secs_f64() < 0.05);
}
