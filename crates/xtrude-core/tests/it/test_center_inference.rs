// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — the centre of a circle or arc on a face's boundary. (The
//! viewport's hover/centre reference tests live with the app.)

use xtrude_core::math::DMat4;
use xtrude_core::snap::{curve_centers_of_face, fit_circle};
use xtrude_core::{DVec3, Mesh};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn circle_pts(cx: f64, cy: f64, r: f64, n: usize) -> Vec<DVec3> {
    (0..n)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            v(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

#[test]
fn test_fit_circle_finds_centre_and_radius_and_rejects_what_is_not_a_circle() {
    let (c, r) = fit_circle(&circle_pts(3.0, 2.0, 1.5, 24), 0.01).unwrap();
    assert!((c - v(3., 2.)).length() < 1e-6 && (r - 1.5).abs() < 1e-6);
    let arc_pts = &circle_pts(3.0, 2.0, 1.5, 24)[..7];
    let (c, r) = fit_circle(arc_pts, 0.01).unwrap();
    assert!((c - v(3., 2.)).length() < 1e-6 && (r - 1.5).abs() < 1e-6);
    assert!(fit_circle(
        &[v(0., 0.), v(1., 0.), v(2., 0.), v(3., 0.), v(3., 1.)],
        0.01
    )
    .is_none());
    assert!(fit_circle(&[v(0., 0.), v(1., 0.)], 0.01).is_none());
}

#[test]
fn test_a_circular_face_gives_its_centre_and_so_does_a_face_with_a_circular_hole() {
    let mut m = Mesh::new();
    let pts = circle_pts(3.0, 2.0, 1.5, 24);
    let f = m.add_face(&pts, &[]);
    m.tag_curve(&pts, true);
    let got = curve_centers_of_face(&m, f, None);
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - v(3., 2.)).length() < 1e-6 && (got[0].1 - 1.5).abs() < 1e-6);

    let mut m2 = Mesh::new();
    let hole = circle_pts(5.0, 5.0, 1.0, 24);
    let g = m2.add_face(
        &[v(0., 0.), v(10., 0.), v(10., 10.), v(0., 10.)],
        std::slice::from_ref(&hole),
    );
    m2.tag_curve(&hole, true);
    let got = curve_centers_of_face(&m2, g, None);
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - v(5., 5.)).length() < 1e-6 && (got[0].1 - 1.0).abs() < 1e-6);
    let sq = m2.add_face(&[v(20., 0.), v(21., 0.), v(21., 1.), v(20., 1.)], &[]);
    assert!(curve_centers_of_face(&m2, sq, None).is_empty());
}

#[test]
fn test_a_components_face_gives_the_placed_centre() {
    let mut m = Mesh::new();
    let pts = circle_pts(0.0, 0.0, 1.0, 24);
    let f = m.add_face(&pts, &[]);
    m.tag_curve(&pts, true);
    let xf = DMat4::from_translation(DVec3::new(10.0, 20.0, 3.0));
    let got = curve_centers_of_face(&m, f, Some(&xf));
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - DVec3::new(10., 20., 3.)).length() < 1e-6);
}

#[test]
fn test_an_imported_circle_without_a_curve_id_gives_its_centre_too() {
    let mut m = Mesh::new();
    let f = m.add_face(&circle_pts(3.0, 2.0, 1.5, 24), &[]);
    for e in m.edge_ids() {
        m.edge_mut(e).soft = true;
    }
    let got = curve_centers_of_face(&m, f, None);
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - v(3., 2.)).length() < 1e-6 && (got[0].1 - 1.5).abs() < 1e-6);
    let mut m2 = Mesh::new();
    let g = m2.add_face(&circle_pts(-4.0, 1.0, 0.8, 16), &[]);
    let got = curve_centers_of_face(&m2, g, None);
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - v(-4., 1.)).length() < 1e-6 && (got[0].1 - 0.8).abs() < 1e-6);
}

#[test]
fn test_a_plate_with_a_quarter_circle_on_its_edge_gives_the_arcs_centre() {
    let mut m = Mesh::new();
    let arc: Vec<DVec3> = (0..=90)
        .step_by(10)
        .map(|t| {
            let a = (t as f64).to_radians();
            v(5. + 2. * a.cos(), 5. + 2. * a.sin())
        })
        .collect();
    let mut lp = vec![v(0., 0.), v(7., 0.)];
    lp.extend(arc);
    lp.push(v(0., 7.));
    let f = m.add_face(&lp, &[]);
    let got = curve_centers_of_face(&m, f, None);
    assert_eq!(got.len(), 1);
    assert!((got[0].0 - v(5., 5.)).length() < 1e-6 && (got[0].1 - 2.0).abs() < 1e-6);
    let sq = m.add_face(&[v(20., 0.), v(30., 0.), v(30., 4.), v(20., 4.)], &[]);
    assert!(curve_centers_of_face(&m, sq, None).is_empty());
}

#[test]
fn test_a_triangle_is_not_a_circle_nor_is_a_trapezoid_from_a_revolve() {
    let mut m = Mesh::new();
    let t = m.add_face(&[v(0., 0.), v(2., 0.), v(1., 1.5)], &[]);
    assert!(curve_centers_of_face(&m, t, None).is_empty());
    let trap = m.add_face(&[v(10., 0.), v(14., 0.), v(13., 2.), v(11., 2.)], &[]);
    for e in m.edge_ids() {
        m.edge_mut(e).soft = true;
    }
    assert!(curve_centers_of_face(&m, trap, None).is_empty());
    let mut m2 = Mesh::new();
    let arc: Vec<DVec3> = [0.0f64, 22.5, 45.0, 67.5, 90.0]
        .iter()
        .map(|a| {
            v(
                5. + 2. * a.to_radians().cos(),
                5. + 2. * a.to_radians().sin(),
            )
        })
        .collect();
    let mut lp = vec![v(0., 0.), v(7., 0.)];
    lp.extend(arc.iter().copied());
    lp.push(v(0., 7.));
    let f = m2.add_face(&lp, &[]);
    let near = |p: DVec3| arc.iter().any(|q| (p - *q).length() < 1e-6);
    for e in m2.edge_ids() {
        let (a, b) = m2.edge_ab(e);
        if near(a) && near(b) {
            m2.edge_mut(e).soft = true;
        }
    }
    assert_eq!(curve_centers_of_face(&m2, f, None).len(), 1);
}
