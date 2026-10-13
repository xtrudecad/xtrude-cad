// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hollowing a solid to a wall thickness.

use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::orient_faces::connected_count;
use xtrude_core::shell::{apply_shell, plan_shell, ShellError};
use xtrude_core::solids::solid_report;
use xtrude_core::{DVec3, FaceId, Mesh};

const MM: f64 = 0.001;

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z) * MM
}

fn cube(s: f64) -> Mesh {
    let mut m = Mesh::new();
    let loops = [
        [p(0., 0., 0.), p(0., s, 0.), p(s, s, 0.), p(s, 0., 0.)],
        [p(0., 0., s), p(s, 0., s), p(s, s, s), p(0., s, s)],
        [p(0., 0., 0.), p(s, 0., 0.), p(s, 0., s), p(0., 0., s)],
        [p(s, 0., 0.), p(s, s, 0.), p(s, s, s), p(s, 0., s)],
        [p(s, s, 0.), p(0., s, 0.), p(0., s, s), p(s, s, s)],
        [p(0., s, 0.), p(0., 0., 0.), p(0., 0., s), p(0., s, s)],
    ];
    for lp in loops {
        m.add_face(&lp, &[]);
    }
    m
}

fn top(m: &Mesh) -> FaceId {
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z > 0.5)
        .unwrap()
}

fn mm3(v: f64) -> f64 {
    v / (MM * MM * MM)
}

#[test]
fn test_an_open_top_box_has_walls_a_floor_and_a_rim() {
    let mut m = cube(20.);
    let all = m.face_ids();
    let lid = top(&m);
    let plan = plan_shell(&m, &all, &[lid], 2. * MM).unwrap();
    apply_shell(&mut m, &plan);
    // 5 outer faces, 5 inner ones and the rim
    assert_eq!(m.faces.len(), 11);
    assert!(is_closed(&m));
    let (solid, _) = solid_report(&m);
    assert!(solid);
    let v = mm3(signed_volume(&m));
    assert!((v - (8000. - 16. * 16. * 18.)).abs() < 1e-6, "{v}");
    let rim = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .expect("the rim");
    assert!(m.face_normal(rim).z > 0.99);
    for q in m
        .face_positions(rim)
        .into_iter()
        .chain(m.face_hole_positions(rim)[0].clone())
    {
        assert!((q.z / MM - 20.).abs() < 1e-9);
    }
    let floor = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            m.face_positions(f)
                .iter()
                .all(|q| (q.z / MM - 2.).abs() < 1e-9)
        })
        .expect("the floor of the cavity");
    // it faces up, into the cavity
    assert!(m.face_normal(floor).z > 0.99);
}

#[test]
fn test_a_closed_hollow_is_two_shells() {
    let mut m = cube(20.);
    let all = m.face_ids();
    let plan = plan_shell(&m, &all, &[], 2. * MM).unwrap();
    apply_shell(&mut m, &plan);
    assert_eq!(m.faces.len(), 12);
    assert!(is_closed(&m));
    assert_eq!(connected_count(&m, all[0]), 6);
    let (solid, _) = solid_report(&m);
    assert!(solid);
    let v = mm3(signed_volume(&m));
    assert!((v - (8000. - 16. * 16. * 16.)).abs() < 1e-6, "{v}");
}

#[test]
fn test_faces_wound_inward_still_hollow_inward() {
    let mut m = cube(20.);
    for f in m.face_ids() {
        m.turn_face_over(f);
    }
    let all = m.face_ids();
    let plan = plan_shell(&m, &all, &[], 2. * MM).unwrap();
    apply_shell(&mut m, &plan);
    let v = mm3(signed_volume(&m)).abs();
    assert!((v - (8000. - 16. * 16. * 16.)).abs() < 1e-6, "{v}");
}

#[test]
fn test_a_wall_thicker_than_half_the_solid_is_refused() {
    let m = cube(20.);
    let all = m.face_ids();
    let lid = top(&m);
    assert_eq!(
        plan_shell(&m, &all, &[lid], 11. * MM).unwrap_err(),
        ShellError::TooThick
    );
    assert_eq!(
        plan_shell(&m, &all, &[], 10. * MM).unwrap_err(),
        ShellError::TooThick
    );
}

#[test]
fn test_what_cannot_be_hollowed_says_why() {
    let m = cube(20.);
    let all = m.face_ids();
    assert_eq!(
        plan_shell(&m, &all, &[], 0.0).unwrap_err(),
        ShellError::NotPositive
    );
    assert_eq!(
        plan_shell(&m, &[], &[], 2. * MM).unwrap_err(),
        ShellError::NoSolid
    );
    assert_eq!(
        plan_shell(&m, &all, &all, 2. * MM).unwrap_err(),
        ShellError::AllOpen
    );
    assert_eq!(
        plan_shell(&m, &all[..5], &[], 2. * MM).unwrap_err(),
        ShellError::NotClosed
    );
    // a corner where four planes meet that no inner point satisfies
    let mut pyramid = Mesh::new();
    let apex = p(3., 5., 10.);
    let base = [
        p(0., 0., 0.),
        p(10., 0., 0.),
        p(10., 10., 0.),
        p(0., 10., 0.),
    ];
    pyramid.add_face(&[base[0], base[3], base[2], base[1]], &[]);
    for i in 0..4 {
        pyramid.add_face(&[base[i], base[(i + 1) % 4], apex], &[]);
    }
    let all = pyramid.face_ids();
    assert!(matches!(
        plan_shell(&pyramid, &all, &[], 1. * MM).unwrap_err(),
        ShellError::NoCorner(_)
    ));
}

/// `outline` (counter-clockwise from above, mm) extruded `h` mm up.
fn prism(outline: &[(f64, f64)], h: f64) -> Mesh {
    let mut m = Mesh::new();
    let lo: Vec<DVec3> = outline.iter().map(|&(x, y)| p(x, y, 0.)).collect();
    let hi: Vec<DVec3> = outline.iter().map(|&(x, y)| p(x, y, h)).collect();
    m.add_face(&lo.iter().rev().copied().collect::<Vec<_>>(), &[]);
    m.add_face(&hi, &[]);
    let n = outline.len();
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_face(&[lo[i], lo[j], hi[j], hi[i]], &[]);
    }
    m
}

#[test]
fn test_a_concave_corner_keeps_the_wall_even() {
    let outline = [
        (0., 0.),
        (20., 0.),
        (20., 10.),
        (10., 10.),
        (10., 20.),
        (0., 20.),
    ];
    let mut m = prism(&outline, 10.);
    assert!(signed_volume(&m) > 0.0);
    let all = m.face_ids();
    let plan = plan_shell(&m, &all, &[], 2. * MM).unwrap();
    apply_shell(&mut m, &plan);
    assert!(is_closed(&m));
    // the cavity: the L shrunk by 2 mm (16 × 6 + 6 × 10 mm²), 6 mm high
    let v = mm3(signed_volume(&m));
    assert!((v - (300. * 10. - 156. * 6.)).abs() < 1e-6, "{v}");
    // the inner corner of the cavity at the L's concave corner
    assert!(m.vertex_at(p(8., 8., 2.)).is_some());
}
