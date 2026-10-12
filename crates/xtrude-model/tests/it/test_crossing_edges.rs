// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Add Crossing Edges: new edges wherever faces cross.

use crate::common;
use common::*;
use xtrude_core::intersect::{cross, world_poly};
use xtrude_core::solids::solid_report;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group, MeshTable};
use xtrude_model::intersect::{segments_for, IntersectMode};

const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];

fn box_mesh(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, color: [f64; 3]) -> Mesh {
    let p = V3;
    let mut m = Mesh::new();
    for pts in [
        [p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0)],
        [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
        [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
        [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
        [p(x1, y1, z0), p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1)],
        [p(x0, y1, z0), p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1)],
    ] {
        let f = m.add_face(&pts, &[]);
        m.face_mut(f).attrs.paint.color = Some(color);
    }
    m
}

fn group_box(t: &mut MeshTable, b: [f64; 6], color: [f64; 3]) -> Group {
    let id = add_mesh(t, box_mesh(b[0], b[1], b[2], b[3], b[4], b[5], color));
    Group::new(id, Some("box"))
}

fn length(segs: &[(DVec3, DVec3)]) -> f64 {
    segs.iter().map(|(p, q)| (*q - *p).length()).sum()
}

#[test]
fn test_two_crossing_squares_meet_on_one_segment() {
    let mut m = Mesh::new();
    let a = m.add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    let b = m.add_face(
        &[
            V3(1., -1., -1.),
            V3(1., 3., -1.),
            V3(1., 3., 1.),
            V3(1., -1., 1.),
        ],
        &[],
    );
    let segs = cross(
        &world_poly(&m, a, None).unwrap(),
        &world_poly(&m, b, None).unwrap(),
    );
    assert_eq!(segs.len(), 1);
    assert!((length(&segs) - 2.0).abs() < 1e-6);
}

#[test]
fn test_a_hole_interrupts_the_crossing() {
    let mut m = Mesh::new();
    let a = m.add_face(
        &[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)],
        &[vec![V(1., 1.), V(1., 3.), V(3., 3.), V(3., 1.)]],
    );
    let b = m.add_face(
        &[
            V3(2., -1., -1.),
            V3(2., 5., -1.),
            V3(2., 5., 1.),
            V3(2., -1., 1.),
        ],
        &[],
    );
    let segs = cross(
        &world_poly(&m, a, None).unwrap(),
        &world_poly(&m, b, None).unwrap(),
    );
    assert_eq!(segs.len(), 2);
    assert!((length(&segs) - 2.0).abs() < 1e-6);
}

#[test]
fn test_coplanar_faces_do_not_cross() {
    let mut m = Mesh::new();
    let a = m.add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    let b = m.add_face(&[V(1., 1.), V(3., 1.), V(3., 3.), V(1., 3.)], &[]);
    assert!(cross(
        &world_poly(&m, a, None).unwrap(),
        &world_poly(&m, b, None).unwrap()
    )
    .is_empty());
}

#[test]
fn test_with_model_two_groups_leave_the_edges_outside_them() {
    let (mut s, mut h) = scene_history();
    let a = group_box(&mut s.doc.meshes, [0., 0., 0., 2., 2., 2.], RED);
    let b = group_box(&mut s.doc.meshes, [1., 1., 1., 3., 3., 3.], BLUE);
    let (aid, amesh) = (a.id, a.mesh);
    s.doc.groups.push(a);
    s.doc.groups.push(b);
    s.selection.insert(EntityRef::Group(aid));
    let segs = segments_for(&s, IntersectMode::Model);
    assert_eq!(segs.len(), 6);
    assert!((length(&segs) - 6.0).abs() < 1e-6);
    let faces_a = s.doc.mesh(amesh).faces.len();
    let mut cmd = build_add_edges(&s, &segs, true, vec![], None);
    h.execute(&mut s, &mut *cmd);
    assert_eq!(nedges(&s), 6);
    assert_eq!(s.doc.mesh(amesh).faces.len(), faces_a);
    h.undo(&mut s);
    assert_eq!(nedges(&s), 0);
}

#[test]
fn test_with_model_splits_the_loose_faces_a_group_runs_through() {
    let (mut s, mut h) = scene_history();
    let src = box_mesh(0., 0., 0., 2., 2., 2., RED);
    for f in src.face_ids() {
        s.mesh_mut().add_face(&src.face_positions(f), &[]);
    }
    let bar = group_box(&mut s.doc.meshes, [1., -1., 1., 1.5, 3., 3.], BLUE);
    s.doc.groups.push(bar);
    let mid = s.active_mesh_id();
    for f in s.mesh().face_ids() {
        s.selection.insert(EntityRef::Face(mid, f));
    }
    let segs = segments_for(&s, IntersectMode::Model);
    let mut cmd = build_add_edges(&s, &segs, true, vec![], None);
    h.execute(&mut s, &mut *cmd);
    assert_eq!(nfaces(&s), 10);
    assert!(solid_report(s.mesh()).0);
}

#[test]
fn test_with_selection_ignores_what_is_not_selected() {
    let (mut s, _h) = scene_history();
    let a = group_box(&mut s.doc.meshes, [0., 0., 0., 2., 2., 2.], RED);
    let b = group_box(&mut s.doc.meshes, [1., 1., 1., 3., 3., 3.], BLUE);
    let c = group_box(&mut s.doc.meshes, [1., -1., 1., 1.5, 3., 3.], BLUE);
    let (aid, bid) = (a.id, b.id);
    s.doc.groups.extend([a, b, c]);
    s.selection.insert(EntityRef::Group(aid));
    s.selection.insert(EntityRef::Group(bid));
    let only_ab = segments_for(&s, IntersectMode::Selection);
    s.selection.clear();
    s.selection.insert(EntityRef::Group(aid));
    let with_model = segments_for(&s, IntersectMode::Model);
    assert_eq!(only_ab.len(), 6);
    assert!(with_model.len() > only_ab.len());
}
