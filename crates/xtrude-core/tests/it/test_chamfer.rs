// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Chamfering the edges of a solid: a one-segment fillet sized by the
//! distance along each face from the edge.

use xtrude_core::fillet::{apply_fillet, plan_chamfer};
use xtrude_core::mesh::EdgeId;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::{DVec3, Mesh};

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

fn edge_at(m: &Mesh, a: DVec3, b: DVec3) -> EdgeId {
    let va = m.vertex_at(a).unwrap();
    let vb = m.vertex_at(b).unwrap();
    m.find_edge(va, vb).unwrap()
}

fn mm3(v: f64) -> f64 {
    v / (MM * MM * MM)
}

#[test]
fn test_a_cube_edge_gets_a_flat_face_as_wide_as_the_distance_across() {
    let mut m = cube(10.);
    let e = edge_at(&m, p(0., 0., 10.), p(10., 0., 10.));
    let plan = plan_chamfer(&m, &[e], 2. * MM).unwrap();
    assert_eq!(plan.new_faces().len(), 1);
    apply_fillet(&mut m, &plan);
    assert_eq!(m.faces.len(), 7);
    assert!(is_closed(&m));
    let bevel = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            let n = m.face_normal(f);
            n.y < -0.5 && n.z > 0.5
        })
        .expect("the chamfer face");
    let pts = m.face_positions(bevel);
    assert_eq!(pts.len(), 4);
    // across the strip: from the top face's cut to the front face's
    let top = pts.iter().find(|q| (q.z - 10. * MM).abs() < 1e-9).unwrap();
    let front = pts
        .iter()
        .find(|q| q.y.abs() < 1e-9 && (q.x - top.x).abs() < 1e-9)
        .unwrap();
    assert!(((*top - *front).length() / MM - 2. * 2f64.sqrt()).abs() < 1e-9);
    assert!((top.y / MM - 2.).abs() < 1e-9);
    assert!((front.z / MM - 8.).abs() < 1e-9);
    let v = mm3(signed_volume(&m));
    assert!((v - (1000. - 2. * 2. / 2. * 10.)).abs() < 1e-6, "{v}");
}

#[test]
fn test_all_twelve_cube_edges_stay_closed() {
    let mut m = cube(10.);
    let plan = plan_chamfer(&m, &m.edge_ids(), 2. * MM).unwrap();
    apply_fillet(&mut m, &plan);
    // 6 cut faces, 12 bevels, 8 corner triangles
    assert_eq!(m.faces.len(), 6 + 12 + 8);
    assert!(is_closed(&m));
    // each edge loses a 2×2/2 prism along its 6 mm middle; in each 2 mm
    // corner cube, the triangle x + y + z = 4 keeps only the far corner's
    // tetrahedron (2³/6)
    let prisms = 12. * 2. * 6.;
    let corners = 8. * (2. * 2. * 2. - 2. * 2. * 2. / 6.);
    let v = mm3(signed_volume(&m));
    assert!((v - (1000. - prisms - corners)).abs() < 1e-6, "{v}");
}

#[test]
fn test_the_distance_is_measured_along_the_faces_at_any_angle() {
    // an equilateral prism: its long edges join faces at 60°
    let h = 10. * 3f64.sqrt() / 2.;
    let (a, b, c) = (p(0., 0., 0.), p(10., 0., 0.), p(5., 0., h));
    let (a2, b2, c2) = (p(0., 20., 0.), p(10., 20., 0.), p(5., 20., h));
    let mut m = Mesh::new();
    for lp in [
        vec![a, b, c],
        vec![a2, c2, b2],
        vec![a, a2, b2, b],
        vec![b, b2, c2, c],
        vec![c, c2, a2, a],
    ] {
        m.add_face(&lp, &[]);
    }
    assert!(signed_volume(&m) > 0.0);
    let e = edge_at(&m, a, a2);
    let plan = plan_chamfer(&m, &[e], 2. * MM).unwrap();
    let bevel = &plan.new_faces()[0];
    for q in bevel {
        // on a face through the edge, 2 mm from it
        assert!((q.x.hypot(q.z) / MM - 2.).abs() < 1e-9, "{q}");
    }
}

#[test]
fn test_a_refusal_speaks_of_the_distance() {
    let m = cube(10.);
    let e = edge_at(&m, p(0., 0., 10.), p(10., 0., 10.));
    let msg = plan_chamfer(&m, &[e], 0.0).unwrap_err();
    assert_eq!(msg, "The distance must be positive.");
    let msg = plan_chamfer(&m, &[], 2. * MM).unwrap_err();
    assert_eq!(msg, "Pick an edge to chamfer.");
    let msg = plan_chamfer(&m, &m.edge_ids(), 6. * MM).unwrap_err();
    assert!(msg.contains("distance is too large"), "{msg}");
}
