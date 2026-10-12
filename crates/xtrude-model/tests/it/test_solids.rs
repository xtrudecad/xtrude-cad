// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Engine tests — Solids tools on two overlapping boxes. (The tool-click tests
//! live with the app tools.)

use crate::common;
use common::*;
use std::collections::HashSet;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_model::group::{Group, MeshTable};
use xtrude_model::solids::{run, solid_volume, SolidOp, SolidOperationCommand};
use xtrude_model::{EntityRef, MeshId};

const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];

fn box_mesh(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, color: [f64; 3]) -> Mesh {
    let mut m = Mesh::new();
    let p = V3;
    let faces = [
        [p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0)],
        [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
        [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
        [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
        [p(x1, y1, z0), p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1)],
        [p(x0, y1, z0), p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1)],
    ];
    for f in faces {
        let id = m.add_face(&f, &[]);
        m.face_mut(id).attrs.paint.color = Some(color);
    }
    m
}

fn group_of(t: &mut MeshTable, m: Mesh, name: &str) -> Group {
    let id = MeshId::fresh();
    t.insert(id, std::sync::Arc::new(m));
    Group::new(id, Some(name))
}

fn pair(t: &mut MeshTable) -> (Group, Group) {
    (
        group_of(t, box_mesh(0., 0., 0., 2., 2., 2., RED), "A"),
        group_of(t, box_mesh(1., 1., 1., 3., 3., 3., BLUE), "B"),
    )
}

fn colours(t: &MeshTable, g: &Group) -> HashSet<Option<[i64; 3]>> {
    let m = &t[&g.mesh];
    m.face_ids()
        .iter()
        .map(|&f| {
            m.face(f)
                .attrs
                .paint
                .color
                .map(|c| c.map(|x| (x * 1e6) as i64))
        })
        .collect()
}

#[test]
fn test_a_closed_box_is_a_solid_with_its_volume() {
    let mut t = MeshTable::new();
    let (a, _) = pair(&mut t);
    assert!((solid_volume(&t, &a).unwrap() - 8.0).abs() < 1e-9);
}

#[test]
fn test_an_open_box_is_not_a_solid() {
    let mut t = MeshTable::new();
    let (a, _) = pair(&mut t);
    let m = std::sync::Arc::make_mut(t.get_mut(&a.mesh).unwrap());
    let f = m.face_ids()[0];
    m.remove_face(f);
    assert!(solid_volume(&t, &a).is_none());
}

#[test]
fn test_nested_groups_disqualify() {
    let mut t = MeshTable::new();
    let (mut a, b) = pair(&mut t);
    a.adopt(vec![b]);
    assert!(solid_volume(&t, &a).is_none());
}

#[test]
fn test_each_operation() {
    let cases: [(SolidOp, Vec<f64>, usize); 6] = [
        (SolidOp::MergeSolids, vec![15.0], 2),
        (SolidOp::Union, vec![15.0], 2),
        (SolidOp::Subtract, vec![7.0], 2),
        (SolidOp::Trim, vec![7.0], 1),
        (SolidOp::Intersect, vec![1.0], 2),
        (SolidOp::Split, vec![7.0, 7.0, 1.0], 2),
    ];
    for (op, volumes, removed) in cases {
        let mut t = MeshTable::new();
        let (a, b) = pair(&mut t);
        let (gone, made) = run(op, &mut t, &[&a, &b]).unwrap();
        assert_eq!(gone.len(), removed, "{op:?}");
        let vols: Vec<f64> = made
            .iter()
            .map(|g| (solid_volume(&t, g).unwrap() * 1e6).round() / 1e6)
            .collect();
        assert_eq!(vols, volumes, "{op:?}");
        assert!(made.iter().all(|g| g.xform.is_none()));
    }
}

#[test]
fn test_subtract_and_trim_cut_the_second_with_the_first() {
    let mut t = MeshTable::new();
    let (a, b) = pair(&mut t);
    let (gone, made) = run(SolidOp::Subtract, &mut t, &[&a, &b]).unwrap();
    assert_eq!(gone, vec![a.id, b.id]);
    let m = &t[&made[0].mesh];
    for v in m.vertex_ids() {
        let p = m.pos(v);
        for c in [p.x, p.y, p.z] {
            assert!((1.0 - 1e-6..=3.0 + 1e-6).contains(&c));
        }
    }
    let (gone, _) = run(SolidOp::Trim, &mut t, &[&a, &b]).unwrap();
    assert_eq!(gone, vec![b.id]);
}

#[test]
fn test_the_faces_of_a_cut_take_the_cutters_material() {
    let mut t = MeshTable::new();
    let (a, b) = pair(&mut t);
    let (_, made) = run(SolidOp::Subtract, &mut t, &[&a, &b]).unwrap();
    let res = &made[0];
    let want: HashSet<Option<[i64; 3]>> = [
        Some(RED.map(|x| (x * 1e6) as i64)),
        Some(BLUE.map(|x| (x * 1e6) as i64)),
    ]
    .into_iter()
    .collect();
    assert_eq!(colours(&t, res), want);
    let m = &t[&res.mesh];
    let red = m
        .face_ids()
        .iter()
        .filter(|&&f| m.face(f).attrs.paint.color == Some(RED))
        .count();
    assert_eq!(red, 3);
}

#[test]
fn test_the_result_has_clean_polygon_faces() {
    let mut t = MeshTable::new();
    let (a, b) = pair(&mut t);
    let (_, made) = run(SolidOp::Subtract, &mut t, &[&a, &b]).unwrap();
    let m = &t[&made[0].mesh];
    assert_eq!(m.faces.len(), 9);
    assert_eq!(
        m.verts.len() as i64 - m.edges.len() as i64 + m.faces.len() as i64,
        2
    );
    assert_eq!(m.verts.len(), 14);
}

#[test]
fn test_merge_solids_drops_an_inner_void_union_keeps_it() {
    let mut t = MeshTable::new();
    let mut outer = box_mesh(0., 0., 0., 4., 4., 4., RED);
    let hollow = box_mesh(1., 1., 1., 3., 3., 3., RED);
    for f in hollow.face_ids() {
        let mut pts = hollow.face_positions(f);
        pts.reverse();
        let id = outer.add_face(&pts, &[]);
        outer.face_mut(id).attrs.paint.color = Some(RED);
    }
    let outer = group_of(&mut t, outer, "cube");
    let other = group_of(&mut t, box_mesh(3., 3., 3., 5., 5., 5., BLUE), "box");
    let (_, u) = run(SolidOp::Union, &mut t, &[&outer, &other]).unwrap();
    let (_, s) = run(SolidOp::MergeSolids, &mut t, &[&outer, &other]).unwrap();
    assert!((solid_volume(&t, &u[0]).unwrap() - (64.0 - 8.0 + 8.0 - 1.0)).abs() < 1e-6 * 63.0);
    assert!((solid_volume(&t, &s[0]).unwrap() - (64.0 + 8.0 - 1.0)).abs() < 1e-6 * 71.0);
}

#[test]
fn test_disjoint_solids_are_refused() {
    let mut t = MeshTable::new();
    let a = group_of(&mut t, box_mesh(0., 0., 0., 1., 1., 1., RED), "a");
    let b = group_of(&mut t, box_mesh(5., 5., 5., 6., 6., 6., BLUE), "b");
    assert!(run(SolidOp::Subtract, &mut t, &[&a, &b]).is_err());
}

#[test]
fn test_a_component_is_read_through_its_placement_and_left_alone() {
    let mut t = MeshTable::new();
    let proto = MeshId::fresh();
    t.insert(
        proto,
        std::sync::Arc::new(box_mesh(0., 0., 0., 2., 2., 2., RED)),
    );
    let mut inst = Group::new(proto, Some("comp"));
    inst.xform = Some(DMat4::from_translation(V3(10., 0., 0.)));
    let cutter = group_of(&mut t, box_mesh(11., 1., 1., 13., 3., 3., BLUE), "cut");
    let before = t[&proto].faces.len();
    let (_, made) = run(SolidOp::Subtract, &mut t, &[&cutter, &inst]).unwrap();
    let res = &made[0];
    assert!(res.xform.is_none() && res.mesh != proto);
    assert_eq!(t[&proto].faces.len(), before);
    let m = &t[&res.mesh];
    assert!(m.vertex_ids().iter().all(|&v| m.pos(v).x >= 10.0 - 1e-6));
}

#[test]
fn test_a_smooth_side_stays_smooth() {
    let mut t = MeshTable::new();
    let mut m = Mesh::new();
    let (n, r, h) = (24usize, 1.0, 2.0);
    let ring: Vec<_> = (0..n)
        .map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            V(r * a.cos(), r * a.sin())
        })
        .collect();
    let top: Vec<_> = ring.iter().map(|&p| p + V3(0., 0., h)).collect();
    let rev: Vec<_> = ring.iter().rev().copied().collect();
    m.add_face(&rev, &[]);
    m.add_face(&top, &[]);
    for k in 0..n {
        m.add_face(&[ring[k], ring[(k + 1) % n], top[(k + 1) % n], top[k]], &[]);
    }
    for k in 0..n {
        let e = m
            .find_edge(m.vertex_at(ring[k]).unwrap(), m.vertex_at(top[k]).unwrap())
            .unwrap();
        m.edge_mut(e).soft = true;
    }
    let cyl = group_of(&mut t, m, "cyl");
    let cutter = group_of(&mut t, box_mesh(0.5, -0.3, -1., 3., 0.3, 3., BLUE), "cut");
    let (_, made) = run(SolidOp::Subtract, &mut t, &[&cutter, &cyl]).unwrap();
    let res = &t[&made[0].mesh];
    assert!(res.edges.iter().filter(|(_, e)| e.soft).count() >= n - 4);
}

#[test]
fn test_one_undo_step_puts_everything_back() {
    let (mut s, mut h) = scene_history();
    let (a, b) = pair(&mut s.doc.meshes);
    let (ia, ib) = (a.id, b.id);
    s.doc.groups.push(a);
    s.doc.groups.push(b);
    h.run(
        &mut s,
        SolidOperationCommand::new(SolidOp::Subtract, vec![ia, ib]),
    );
    assert_eq!(s.doc.groups.len(), 1);
    let only: HashSet<EntityRef> = s
        .doc
        .groups
        .iter()
        .map(|g| EntityRef::Group(g.id))
        .collect();
    assert_eq!(s.selection.iter().copied().collect::<HashSet<_>>(), only);
    h.undo(&mut s);
    assert_eq!(
        s.doc.groups.iter().map(|g| g.id).collect::<Vec<_>>(),
        vec![ia, ib]
    );
    h.redo(&mut s);
    assert_eq!(s.doc.groups.len(), 1);
}
