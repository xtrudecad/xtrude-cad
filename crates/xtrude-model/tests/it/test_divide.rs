// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Divide and the undo cap.

use crate::common;
use common::*;
use std::collections::HashMap;
use xtrude_core::mesh::EdgeId;
use xtrude_core::Mesh;
use xtrude_model::commands::AddEdgeCommand;
use xtrude_model::edits::divide_edges;
use xtrude_model::history::FnCommand;

fn xs(m: &Mesh) -> Vec<f64> {
    let mut v: Vec<f64> = m
        .vertex_ids()
        .iter()
        .map(|&v| (m.pos(v).x * 1e6).round() / 1e6)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn curves(m: &Mesh) -> HashMap<i64, Vec<EdgeId>> {
    let mut out: HashMap<i64, Vec<EdgeId>> = HashMap::new();
    for e in m.edge_ids() {
        if let Some(c) = m.edge(e).curve {
            out.entry(c).or_default().push(e);
        }
    }
    out
}

fn length(m: &Mesh, es: &[EdgeId]) -> f64 {
    es.iter().map(|&e| m.edge_length(e)).sum()
}

#[test]
fn test_a_line_is_divided_into_equal_pieces() {
    let mut m = Mesh::new();
    let e = m.add_edge(V(0., 0.), V(4., 0.)).unwrap();
    assert_eq!(divide_edges(&mut m, &[e], 4), 3);
    assert_eq!(xs(&m), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_eq!(m.edges.len(), 4);
}

#[test]
fn test_a_face_edge_divided_keeps_the_face_whole() {
    let mut m = Mesh::new();
    let f = m.add_face(&[V(0., 0.), V(4., 0.), V(4., 2.), V(0., 2.)], &[]);
    let bottom = m
        .edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = m.edge_ab(e);
            a.y.abs() < 1e-9 && b.y.abs() < 1e-9
        })
        .unwrap();
    divide_edges(&mut m, &[bottom], 2);
    assert_eq!(m.faces.len(), 1);
    assert_eq!(m.face(f).outer.len(), 5);
}

#[test]
fn test_an_arc_divided_becomes_n_independent_arcs_of_equal_length() {
    let mut m = Mesh::new();
    let pts = [V(0., 0.), V(1., 0.5), V(2., 0.8), V(3., 0.5), V(4., 0.)];
    for w in pts.windows(2) {
        m.add_edge(w[0], w[1]).unwrap();
    }
    m.tag_curve(&pts, false);
    let seed = m
        .edge_ids()
        .into_iter()
        .find(|&e| m.edge(e).curve.is_some())
        .unwrap();
    let total = length(&m, &m.edge_ids());
    assert_eq!(divide_edges(&mut m, &[seed], 3), 2);
    let cs = curves(&m);
    assert_eq!(cs.len(), 3);
    assert_eq!(cs.values().map(|v| v.len()).sum::<usize>(), 6);
    for es in cs.values() {
        assert!((length(&m, es) - total / 3.0).abs() < 1e-6);
    }
    let soft = m.edge(seed).soft;
    assert!(cs.values().flatten().all(|&e| m.edge(e).soft == soft));
}

#[test]
fn test_a_facet_running_against_the_walk_is_cut_at_the_right_place() {
    let mut m = Mesh::new();
    m.add_edge(V(0., 0.), V(1., 0.)).unwrap();
    m.add_edge(V(2., 0.), V(1., 0.)).unwrap();
    m.add_edge(V(2., 0.), V(3., 0.)).unwrap();
    m.tag_curve(&[V(0., 0.), V(1., 0.), V(2., 0.), V(3., 0.)], false);
    let seed = m
        .edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = m.edge_ab(e);
            a.x.abs() < 1e-9 || b.x.abs() < 1e-9
        })
        .unwrap();
    divide_edges(&mut m, &[seed], 2);
    assert_eq!(xs(&m), vec![0.0, 1.0, 1.5, 2.0, 3.0]);
    let cs = curves(&m);
    assert_eq!(cs.len(), 2);
    assert!(cs.values().all(|es| (length(&m, es) - 1.5).abs() < 1e-6));
}

fn circle(n: usize) -> Vec<xtrude_core::DVec3> {
    (0..n)
        .map(|i| {
            let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            V(t.cos(), t.sin())
        })
        .collect()
}

#[test]
fn test_a_circle_divided_where_the_marks_fall_on_facet_vertices() {
    let mut m = Mesh::new();
    let pts = circle(24);
    for i in 0..24 {
        m.add_edge(pts[i], pts[(i + 1) % 24]).unwrap();
    }
    m.tag_curve(&pts, true);
    let seed = m.edge_ids()[0];
    assert_eq!(divide_edges(&mut m, &[seed], 6), 0);
    let cs = curves(&m);
    assert_eq!(cs.len(), 6);
    let mut sizes: Vec<usize> = cs.values().map(|v| v.len()).collect();
    sizes.sort();
    assert_eq!(sizes, vec![4; 6]);
    assert_eq!(m.edges.len(), 24);
}

#[test]
fn test_a_circle_divided_in_four_is_four_quarter_arcs() {
    let mut m = Mesh::new();
    let pts = circle(18);
    for i in 0..18 {
        m.add_edge(pts[i], pts[(i + 1) % 18]).unwrap();
    }
    m.tag_curve(&pts, true);
    let total = length(&m, &m.edge_ids());
    let e5 = m.edge_ids()[5];
    divide_edges(&mut m, &[e5], 4);
    let cs = curves(&m);
    assert_eq!(cs.len(), 4);
    for es in cs.values() {
        assert!((length(&m, es) - total / 4.0).abs() < 1e-6);
    }
    let mut m2 = Mesh::new();
    let f = m2.add_face(&pts, &[]);
    m2.tag_curve(&pts, true);
    let e0 = m2.edge_ids()[0];
    divide_edges(&mut m2, &[e0], 4);
    assert_eq!(m2.faces.len(), 1);
    assert_eq!(m2.face(f).outer.len(), 20);
}

#[test]
fn test_divide_is_one_undo_step() {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(3., 0.)));
    let e = s.mesh().edge_ids()[0];
    h.run(
        &mut s,
        FnCommand::new("SnapshotMutation", move |sc: &mut xtrude_model::Scene| {
            divide_edges(sc.mesh_mut(), &[e], 3);
            Ok(())
        }),
    );
    assert_eq!(nedges(&s), 3);
    h.undo(&mut s);
    assert_eq!(nedges(&s), 1);
}

#[test]
fn test_the_undo_stack_is_capped() {
    let (mut s, mut h) = scene_history();
    h.max_steps = 3;
    for i in 0..6 {
        h.run(
            &mut s,
            AddEdgeCommand::new(V(i as f64, 0.), V(i as f64 + 1., 0.)),
        );
    }
    assert_eq!(h.undo_len(), 3);
    h.max_steps = 0;
    for i in 6..10 {
        h.run(
            &mut s,
            AddEdgeCommand::new(V(i as f64, 0.), V(i as f64 + 1., 0.)),
        );
    }
    assert_eq!(h.undo_len(), 7);
}
