// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The shared-vertex, non-manifold mesh.
//! (`test_mesh_is_read_compatible_with_igz_save` lives in xtrude-io.)

use std::collections::HashSet;

use xtrude_core::mesh::{MeshError, VertexId};
use xtrude_core::{DVec3, Mesh};

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

// Named like the vector it builds.
#[allow(non_snake_case)]
fn V3(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn vat(m: &Mesh, p: DVec3) -> VertexId {
    m.vertex_at(p).expect("vertex")
}

// ---- welding ----------------------------------------------------------------

#[test]
fn test_coincident_positions_weld_to_one_vertex() {
    let mut m = Mesh::new();
    let a = m.vertex(V3(1.0, 1.0, 1.0));
    let b = m.vertex(V3(1.0, 1.0, 1.0));
    assert_eq!(a, b);
    assert_eq!(m.verts.len(), 1);
}

#[test]
fn test_weld_within_tolerance() {
    let mut m = Mesh::new();
    let a = m.vertex(V(1.0, 0.0));
    let b = m.vertex(V(1.00001, 0.0));
    assert_eq!(a, b);
}

#[test]
fn test_two_edges_sharing_a_corner_share_the_vertex() {
    let mut m = Mesh::new();
    let e1 = m.add_edge(V(0.0, 0.0), V(1.0, 0.0)).unwrap();
    let e2 = m.add_edge(V(1.0, 0.0), V(1.0, 1.0)).unwrap();
    assert_eq!(m.edge(e1).v1, m.edge(e2).v0);
    assert_eq!(m.verts.len(), 3);
    let got: HashSet<_> = m.verts[m.edge(e1).v1].edges.iter().copied().collect();
    assert_eq!(got, HashSet::from([e1, e2]));
}

#[test]
fn test_add_edge_dedups() {
    let mut m = Mesh::new();
    let e1 = m.add_edge(V(0.0, 0.0), V(2.0, 0.0)).unwrap();
    let e2 = m.add_edge(V(2.0, 0.0), V(0.0, 0.0)).unwrap();
    assert_eq!(e1, e2);
    assert_eq!(m.edges.len(), 1);
}

#[test]
fn test_degenerate_edge_rejected() {
    let mut m = Mesh::new();
    assert_eq!(
        m.add_edge(V(0.0, 0.0), V(0.0, 0.0)),
        Err(MeshError::DegenerateEdge)
    );
}

// ---- radial incidence (non-manifold) ----------------------------------------

#[test]
fn test_edge_between_two_faces_has_both() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    let shared = m
        .find_edge(vat(&m, V(1.0, 0.0)), vat(&m, V(1.0, 1.0)))
        .unwrap();
    assert_eq!(m.edge(shared).faces.len(), 2);
}

#[test]
fn test_non_manifold_edge_carries_three_faces() {
    let mut m = Mesh::new();
    let (e0, e1) = (V3(0.0, 0.0, 0.0), V3(0.0, 0.0, 1.0));
    m.add_face(&[e0, e1, V3(1.0, 0.0, 1.0), V3(1.0, 0.0, 0.0)], &[]);
    m.add_face(&[e0, e1, V3(0.0, 1.0, 1.0), V3(0.0, 1.0, 0.0)], &[]);
    m.add_face(&[e0, e1, V3(-1.0, 0.0, 1.0), V3(-1.0, 0.0, 0.0)], &[]);
    let shared = m.find_edge(vat(&m, e0), vat(&m, e1)).unwrap();
    assert_eq!(m.edge(shared).faces.len(), 3);
}

#[test]
fn test_remove_face_detaches_incidence() {
    let mut m = Mesh::new();
    let f1 = m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    let shared = m
        .find_edge(vat(&m, V(1.0, 0.0)), vat(&m, V(1.0, 1.0)))
        .unwrap();
    assert_eq!(m.edge(shared).faces.len(), 2);
    m.remove_face(f1);
    assert!(!m.has_face(f1));
    assert_eq!(m.edge(shared).faces.len(), 1);
    assert!(m.has_edge(shared));
}

#[test]
fn test_vertex_faces_query() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    let corner = vat(&m, V(1.0, 0.0));
    assert_eq!(m.vertex_faces(corner).len(), 2);
}

// ---- the headline: move follows for free ------------------------------------

#[test]
fn test_move_vertex_drags_all_incident_geometry() {
    let mut m = Mesh::new();
    let left = m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    let right = m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    let a = vat(&m, V(1.0, 0.0));
    m.move_vertex(a, V3(0.0, 0.0, 1.0));
    let b = vat(&m, V(1.0, 1.0));
    m.move_vertex(b, V3(0.0, 0.0, 1.0));
    let raised = |m: &Mesh, f| -> HashSet<(i64, i64, i64)> {
        m.face_positions(f)
            .into_iter()
            .filter(|p| (p.z - 1.0).abs() < 1e-6)
            .map(|p| (p.x.round() as i64, p.y.round() as i64, p.z.round() as i64))
            .collect()
    };
    assert_eq!(raised(&m, left), HashSet::from([(1, 0, 1), (1, 1, 1)]));
    assert_eq!(raised(&m, right), HashSet::from([(1, 0, 1), (1, 1, 1)]));
}

#[test]
fn test_move_vertex_rekeys_registry() {
    let mut m = Mesh::new();
    let v = m.vertex(V3(0.0, 0.0, 0.0));
    m.move_vertex(v, V3(5.0, 0.0, 0.0));
    assert_eq!(m.vertex_at(V3(0.0, 0.0, 0.0)), None);
    assert_eq!(m.vertex_at(V3(5.0, 0.0, 0.0)), Some(v));
    assert_eq!(m.vertex(V3(5.0, 0.0, 0.0)), v);
}

// ---- face geometry ----------------------------------------------------------

#[test]
fn test_face_geometry_basics() {
    let mut m = Mesh::new();
    let f = m.add_face(&[V(0.0, 0.0), V(2.0, 0.0), V(2.0, 2.0), V(0.0, 2.0)], &[]);
    assert!((m.face_area(f) - 4.0).abs() < 1e-6);
    let n = m.face_normal(f);
    assert!(n.x.abs() < 1e-6 && n.y.abs() < 1e-6 && (n.z.abs() - 1.0).abs() < 1e-6);
    let c = m.face_centroid(f);
    assert!((c.x - 1.0).abs() < 1e-6 && (c.y - 1.0).abs() < 1e-6);
    assert_eq!(m.face_triangulate(f, None).len(), 2);
}

#[test]
fn test_add_face_creates_boundary_edges_and_reuses_them() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    assert_eq!(m.edges.len(), 4);
    m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    assert_eq!(m.edges.len(), 7);
}

#[test]
fn test_mesh_face_exposes_positions_like_legacy() {
    let mut m = Mesh::new();
    let f = m.add_face(
        &[V(0.0, 0.0), V(2.0, 0.0), V(2.0, 2.0), V(0.0, 2.0)],
        &[vec![V(0.5, 0.5), V(1.5, 0.5), V(1.5, 1.5), V(0.5, 1.5)]],
    );
    assert_eq!(m.face_positions(f).len(), 4);
    assert_eq!(m.face_hole_positions(f)[0].len(), 4);
    assert_eq!(m.face(f).holes[0].len(), 4);
    assert!(m.face_triangulate(f, None).len() > 2);
}

// ---- M2: split_edge ----------------------------------------------------------

#[test]
fn test_split_edge_propagates_to_both_faces() {
    let mut m = Mesh::new();
    let left = m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    let right = m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    let shared = m
        .find_edge(vat(&m, V(1.0, 0.0)), vat(&m, V(1.0, 1.0)))
        .unwrap();
    let (e0, e1) = m.split_edge(shared, V(1.0, 0.5));
    let mid = vat(&m, V(1.0, 0.5));
    assert!(m.face(left).outer.contains(&mid) && m.face(right).outer.contains(&mid));
    assert_eq!(m.face(left).outer.len(), 5);
    assert_eq!(m.face(right).outer.len(), 5);
    assert!(!m.has_edge(shared));
    let want = HashSet::from([left, right]);
    assert_eq!(
        m.edge(e0).faces.iter().copied().collect::<HashSet<_>>(),
        want
    );
    assert_eq!(
        m.edge(e1).faces.iter().copied().collect::<HashSet<_>>(),
        want
    );
}

#[test]
fn test_split_edge_non_manifold_propagates_to_all() {
    let mut m = Mesh::new();
    let (a, b) = (V3(0.0, 0.0, 0.0), V3(0.0, 0.0, 2.0));
    m.add_face(&[a, b, V3(1.0, 0.0, 2.0), V3(1.0, 0.0, 0.0)], &[]);
    m.add_face(&[a, b, V3(0.0, 1.0, 2.0), V3(0.0, 1.0, 0.0)], &[]);
    m.add_face(&[a, b, V3(-1.0, 0.0, 2.0), V3(-1.0, 0.0, 0.0)], &[]);
    let e = m.find_edge(vat(&m, a), vat(&m, b)).unwrap();
    let (e0, e1) = m.split_edge(e, V3(0.0, 0.0, 1.0));
    let mid = vat(&m, V3(0.0, 0.0, 1.0));
    let faces = m.vertex_faces(mid);
    assert!(faces.iter().all(|&f| m.face(f).outer.len() == 5));
    assert_eq!(faces.len(), 3);
    assert_eq!(m.edge(e0).faces.len(), 3);
    assert_eq!(m.edge(e1).faces.len(), 3);
}

#[test]
fn test_split_edge_then_move_forms_a_gable() {
    let mut m = Mesh::new();
    let left = m.add_face(
        &[
            V3(0.0, 0.0, 2.0),
            V3(2.0, 0.0, 2.0),
            V3(2.0, 2.0, 2.0),
            V3(0.0, 2.0, 2.0),
        ],
        &[],
    );
    let right = m.add_face(
        &[
            V3(2.0, 0.0, 2.0),
            V3(4.0, 0.0, 2.0),
            V3(4.0, 2.0, 2.0),
            V3(2.0, 2.0, 2.0),
        ],
        &[],
    );
    let ridge = m
        .find_edge(vat(&m, V3(2.0, 0.0, 2.0)), vat(&m, V3(2.0, 2.0, 2.0)))
        .unwrap();
    m.split_edge(ridge, V3(2.0, 1.0, 2.0));
    let apex = vat(&m, V3(2.0, 1.0, 2.0));
    m.move_vertex(apex, V3(0.0, 0.0, 1.0));
    assert!(m
        .face_positions(left)
        .iter()
        .any(|p| (p.z - 3.0).abs() < 1e-6));
    assert!(m
        .face_positions(right)
        .iter()
        .any(|p| (p.z - 3.0).abs() < 1e-6));
}

// ---- weld_coincident ----------------------------------------------------------

#[test]
fn test_weld_coincident_merges_translated_duplicates() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(2.0, 0.0), V(3.0, 0.0), V(3.0, 1.0), V(2.0, 1.0)], &[]);
    for v in m.vertex_ids() {
        if m.pos(v).x >= 2.0 {
            m.move_vertex(v, V3(-1.0, 0.0, 0.0));
        }
    }
    assert_eq!(m.verts.len(), 8);
    assert!(m.weld_coincident());
    assert_eq!(m.verts.len(), 6);
    let (va, vb) = (vat(&m, V(1.0, 0.0)), vat(&m, V(1.0, 1.0)));
    let border = m.find_edge(va, vb).unwrap();
    assert_eq!(m.edge(border).faces.len(), 2);
}

#[test]
fn test_weld_coincident_drops_zero_area_faces_and_edges() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    for v in m.vertex_ids() {
        if m.pos(v).y == 1.0 {
            m.move_vertex(v, V3(0.0, -1.0, 0.0));
        }
    }
    m.weld_coincident();
    assert_eq!(m.faces.len(), 0);
    assert_eq!(m.verts.len(), 2);
    assert_eq!(m.edges.len(), 1);
}

#[test]
fn test_weld_coincident_noop_on_clean_mesh() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    assert!(!m.weld_coincident());
    assert_eq!((m.verts.len(), m.edges.len(), m.faces.len()), (4, 4, 1));
}

#[test]
fn test_dedupe_faces_drops_identical_cycle() {
    let mut m = Mesh::new();
    let a = m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    assert_eq!(m.faces.len(), 2);
    assert_eq!(m.dedupe_faces(), 1);
    assert_eq!(m.face_ids(), vec![a]);
}

#[test]
fn test_dedupe_faces_keeps_distinct_faces() {
    let mut m = Mesh::new();
    m.add_face(&[V(0.0, 0.0), V(1.0, 0.0), V(1.0, 1.0), V(0.0, 1.0)], &[]);
    m.add_face(&[V(1.0, 0.0), V(2.0, 0.0), V(2.0, 1.0), V(1.0, 1.0)], &[]);
    assert_eq!(m.dedupe_faces(), 0);
    assert_eq!(m.faces.len(), 2);
}
