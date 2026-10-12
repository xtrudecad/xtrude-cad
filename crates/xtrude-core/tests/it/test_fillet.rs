// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rounding the edges of a solid.

use std::collections::HashSet;

use xtrude_core::fillet::{apply_fillet, plan_fillet};
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::mesh::EdgeId;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::{DVec3, Mesh};

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn boxm(l: f64, w: f64, h: f64) -> Mesh {
    let mut m = Mesh::new();
    let loops = [
        [p(0., 0., 0.), p(0., w, 0.), p(l, w, 0.), p(l, 0., 0.)],
        [p(0., 0., h), p(l, 0., h), p(l, w, h), p(0., w, h)],
        [p(0., 0., 0.), p(l, 0., 0.), p(l, 0., h), p(0., 0., h)],
        [p(l, 0., 0.), p(l, w, 0.), p(l, w, h), p(l, 0., h)],
        [p(l, w, 0.), p(0., w, 0.), p(0., w, h), p(l, w, h)],
        [p(0., w, 0.), p(0., 0., 0.), p(0., 0., h), p(0., w, h)],
    ];
    for lp in loops {
        m.add_face(&lp, &[]);
    }
    m
}

fn edge_at(m: &Mesh, a: (f64, f64, f64), b: (f64, f64, f64)) -> EdgeId {
    let va = m.vertex_at(p(a.0, a.1, a.2)).unwrap();
    let vb = m.vertex_at(p(b.0, b.1, b.2)).unwrap();
    m.find_edge(va, vb).unwrap()
}

fn rounded_off(r: f64, length: f64) -> f64 {
    (r * r - std::f64::consts::PI * r * r / 4.0) * length
}

fn faces_all(m: &Mesh, pred: impl Fn(&DVec3) -> bool) -> Vec<xtrude_core::FaceId> {
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(&pred))
        .collect()
}

#[test]
fn test_one_edge_becomes_a_tangent_strip_and_the_caps_get_the_arc() {
    let mut m = boxm(2., 1., 1.);
    let e = edge_at(&m, (0., 0., 1.), (2., 0., 1.));
    let plan = plan_fillet(&m, &[e], 0.2, 8).unwrap();
    apply_fillet(&mut m, &plan);
    assert_eq!(m.faces.len(), 6 + 8);
    assert!(is_closed(&m));
    for &f in &m.face_ids()[6..] {
        for q in m.face_positions(f) {
            assert!(((q.y - 0.2).hypot(q.z - 0.8) - 0.2).abs() < 1e-6);
        }
    }
    let top = faces_all(&m, |q| (q.z - 1.0).abs() < 1e-9);
    assert_eq!(top.len(), 1);
    let miny = m
        .face_positions(top[0])
        .iter()
        .map(|q| q.y)
        .fold(f64::INFINITY, f64::min);
    assert!((miny - 0.2).abs() < 1e-9);
    let front = faces_all(&m, |q| q.y.abs() < 1e-9);
    assert_eq!(front.len(), 1);
    let maxz = m
        .face_positions(front[0])
        .iter()
        .map(|q| q.z)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((maxz - 0.8).abs() < 1e-9);
    let caps = faces_all(&m, |q| q.x.abs() < 1e-9);
    assert_eq!(caps.len(), 1);
    assert_eq!(m.face(caps[0]).outer.len(), 3 + 9);
    let vol = signed_volume(&m).abs();
    let want = 2.0 - rounded_off(0.2, 2.0);
    assert!(want - 0.002 < vol && vol < want + 0.002);
    let soft = m.edges.iter().filter(|(_, e)| e.soft).count();
    let curves: HashSet<i64> = m.edges.iter().filter_map(|(_, e)| e.curve).collect();
    assert_eq!(soft, 9);
    assert_eq!(curves.len(), 2);
    assert!(m.edges.iter().all(|(_, e)| e.curve.is_none() || !e.soft));
}

#[test]
fn test_the_top_rim_is_a_chain_with_mitred_joins() {
    let mut m = boxm(2., 1., 1.);
    let rim = [
        edge_at(&m, (0., 0., 1.), (2., 0., 1.)),
        edge_at(&m, (2., 0., 1.), (2., 1., 1.)),
        edge_at(&m, (2., 1., 1.), (0., 1., 1.)),
        edge_at(&m, (0., 1., 1.), (0., 0., 1.)),
    ];
    let plan = plan_fillet(&m, &rim, 0.2, 8).unwrap();
    apply_fillet(&mut m, &plan);
    assert_eq!(m.faces.len(), 6 + 4 * 8);
    assert!(is_closed(&m));
    let top = faces_all(&m, |q| (q.z - 1.0).abs() < 1e-9);
    assert_eq!(top.len(), 1);
    let mut xs: Vec<f64> = m.face_positions(top[0]).iter().map(|q| q.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!((xs[0] - 0.2).abs() < 1e-9 && (xs[xs.len() - 1] - 1.8).abs() < 1e-9);
    let lf: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            a.x.abs() < 1e-9 && b.x.abs() < 1e-9 && a.y.abs() < 1e-9 && b.y.abs() < 1e-9
        })
        .collect();
    assert_eq!(lf.len(), 1);
    let (a, b) = m.edge_ab(lf[0]);
    assert!((a.z.max(b.z) - 0.8).abs() < 1e-9);
    assert!(!m.edges.iter().any(|(_, e)| e.curve.is_some()));
}

#[test]
fn test_all_twelve_edges_add_spherical_corners() {
    let mut m = boxm(2., 1., 1.);
    let all = m.edge_ids();
    let plan = plan_fillet(&m, &all, 0.2, 8).unwrap();
    apply_fillet(&mut m, &plan);
    assert_eq!(m.faces.len(), 6 + 12 * 8 + 8 * 64);
    assert!(is_closed(&m));
    let centre = p(0.2, 0.2, 0.2);
    let near: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let ps = m.face_positions(f);
            ps.len() == 3 && ps.iter().all(|q| q.x < 0.21 && q.y < 0.21 && q.z < 0.21)
        })
        .collect();
    assert_eq!(near.len(), 64);
    for f in near {
        for q in m.face_positions(f) {
            assert!(((q - centre).length() - 0.2).abs() < 1e-6);
        }
    }
    assert!(m.edges.iter().all(|(_, e)| e.soft));
    let vol = signed_volume(&m).abs();
    assert!(1.80 < vol && vol < 1.90);
}

#[test]
fn test_a_concave_edge_rounds_into_the_hollow() {
    let mut m = Mesh::new();
    let foot = [
        p(0., 0., 0.),
        p(2., 0., 0.),
        p(2., 1., 0.),
        p(1., 1., 0.),
        p(1., 2., 0.),
        p(0., 2., 0.),
    ];
    let rev: Vec<DVec3> = foot.iter().rev().copied().collect();
    m.add_face(&rev, &[]);
    let top: Vec<DVec3> = foot.iter().map(|q| p(q.x, q.y, 1.0)).collect();
    m.add_face(&top, &[]);
    let n = foot.len();
    for i in 0..n {
        let (a, b) = (foot[i], foot[(i + 1) % n]);
        m.add_face(&[a, b, p(b.x, b.y, 1.0), p(a.x, a.y, 1.0)], &[]);
    }
    assert!(is_closed(&m));
    let v0 = signed_volume(&m).abs();
    let e = edge_at(&m, (1., 1., 0.), (1., 1., 1.));
    let plan = plan_fillet(&m, &[e], 0.2, 8).unwrap();
    apply_fillet(&mut m, &plan);
    assert!(is_closed(&m));
    assert!(signed_volume(&m).abs() > v0 + 0.5 * rounded_off(0.2, 1.0));
    for &f in &m.face_ids()[8..] {
        for q in m.face_positions(f) {
            assert!(((q.x - 1.2).hypot(q.y - 1.2) - 0.2).abs() < 1e-6);
        }
    }
}

#[test]
fn test_an_oblique_end_cap_is_cut_on_its_own_plane() {
    let mut m = Mesh::new();
    let loops = [
        vec![p(0., 0., 0.), p(0., 1., 0.), p(2., 1., 0.), p(2.5, 0., 0.)],
        vec![p(0., 0., 1.), p(2.5, 0., 1.), p(2., 1., 1.), p(0., 1., 1.)],
        vec![p(0., 0., 0.), p(2.5, 0., 0.), p(2.5, 0., 1.), p(0., 0., 1.)],
        vec![p(2.5, 0., 0.), p(2., 1., 0.), p(2., 1., 1.), p(2.5, 0., 1.)],
        vec![p(2., 1., 0.), p(0., 1., 0.), p(0., 1., 1.), p(2., 1., 1.)],
        vec![p(0., 1., 0.), p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.)],
    ];
    for lp in loops {
        m.add_face(&lp, &[]);
    }
    let e = edge_at(&m, (0., 0., 1.), (2.5, 0., 1.));
    let plan = plan_fillet(&m, &[e], 0.2, 8).unwrap();
    apply_fillet(&mut m, &plan);
    assert!(is_closed(&m));
    let cap: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let ps = m.face_positions(f);
            ps.len() > 4 && ps.iter().all(|q| (q.x + 0.5 * q.y - 2.5).abs() < 1e-6)
        })
        .collect();
    assert_eq!(cap.len(), 1);
}

#[test]
fn test_refusals_leave_the_mesh_untouched() {
    let mut m = boxm(2., 1., 1.);
    let (faces, edges) = (m.faces.len(), m.edges.len());
    let all = m.edge_ids();
    let msg = plan_fillet(&m, &all, 0.6, 8).unwrap_err();
    assert!(msg.contains("too large"), "{msg}");
    m.add_edge(p(5., 5., 5.), p(6., 5., 5.)).unwrap();
    let lone = edge_at(&m, (5., 5., 5.), (6., 5., 5.));
    assert!(plan_fillet(&m, &[lone], 0.1, 8).is_err());
    m.remove_edge(lone);
    m.add_face(
        &[p(0., 0., 1.), p(0., 0., 2.), p(0., -1., 2.), p(0., -1., 1.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 1.), p(1., 0., 1.), p(1., 0., 2.), p(0., 0., 2.)],
        &[],
    );
    let four = [
        edge_at(&m, (0., 0., 1.), (2., 0., 1.)),
        edge_at(&m, (0., 0., 1.), (0., 1., 1.)),
        edge_at(&m, (0., 0., 1.), (0., 0., 0.)),
        edge_at(&m, (0., 0., 1.), (0., 0., 2.)),
    ];
    assert!(plan_fillet(&m, &four, 0.1, 8).is_err());
    assert!(plan_fillet(&m, &[], 0.1, 8).is_err());
    assert!(plan_fillet(&m, &[edge_at(&m, (2., 0., 1.), (2., 1., 1.))], 0.0, 8).is_err());
    assert_eq!(m.faces.len(), faces + 2);
    assert_eq!(m.edges.len(), edges + 7);
}

#[test]
fn test_the_strip_inherits_a_material_shared_by_both_faces() {
    let col = [0.5, 0.2, 0.2];
    let mut m = boxm(2., 1., 1.);
    for f in m.face_ids() {
        m.face_mut(f).attrs.paint.color = Some(col);
    }
    let e = edge_at(&m, (0., 0., 1.), (2., 0., 1.));
    let plan = plan_fillet(&m, &[e], 0.2, 4).unwrap();
    apply_fillet(&mut m, &plan);
    assert!(m
        .face_ids()
        .iter()
        .all(|&f| m.face(f).attrs.paint.color == Some(col)));
    let mut m = boxm(2., 1., 1.);
    let top = m.face_ids()[1];
    m.face_mut(top).attrs.paint.color = Some([0.1, 0.9, 0.1]);
    let e = edge_at(&m, (0., 0., 1.), (2., 0., 1.));
    let plan = plan_fillet(&m, &[e], 0.2, 4).unwrap();
    apply_fillet(&mut m, &plan);
    assert!(m.face_ids()[6..]
        .iter()
        .all(|&f| m.face(f).attrs.is_empty()));
}

#[test]
fn test_a_turned_and_squashed_cube_rounds_all_twelve_edges() {
    let xf = DMat4::from_translation(p(8.5, -3.0, 0.4))
        * DMat4::from_scale(p(1.0, 1.0, 0.5))
        * xtrude_core::math::rotation_deg(p(0.3, 1.0, 0.45), 33.0);
    let src = boxm(2., 2., 2.);
    let mut rng = Mt19937::new(74);
    for _ in 0..5 {
        let mut m = Mesh::new();
        for f in src.face_ids() {
            let lp: Vec<DVec3> = src
                .face_positions(f)
                .into_iter()
                .map(|v| xf.project_point(v))
                .collect();
            m.add_face(&lp, &[]);
        }
        let mut edges = m.edge_ids();
        rng.shuffle(&mut edges);
        let before = signed_volume(&m).abs();
        let plan = plan_fillet(&m, &edges, 0.2, 8).unwrap();
        apply_fillet(&mut m, &plan);
        assert!(is_closed(&m));
        let after = signed_volume(&m).abs();
        assert!(0.85 * before < after && after < before);
    }
}
