// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Make Independent keeps subgroups as subgroups, with a private copy of the whole
//! tree.

use crate::common;
use common::*;
use std::sync::Arc;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_model::commands::MakeIndependentCommand;
use xtrude_model::group::{add_mesh, copy_group, world_mesh, Group, MeshTable};
use xtrude_model::ids::GroupId;

fn quad(z: f64) -> Mesh {
    let mut m = Mesh::new();
    m.add_face(
        &[V3(0., 0., z), V3(1., 0., z), V3(1., 1., z), V3(0., 1., z)],
        &[],
    );
    m
}

fn placed(t: &mut MeshTable, m: Mesh, dx: f64, dz: f64) -> Group {
    let mut g = Group::new(add_mesh(t, m), None);
    g.xform = Some(DMat4::from_translation(V3(dx, 0., dz)));
    g
}

fn points(t: &MeshTable, g: &Group) -> Vec<(i64, i64, i64)> {
    let mut v: Vec<_> = world_mesh(t, g)
        .verts
        .iter()
        .map(|(_, v)| {
            let p = v.position;
            (
                (p.x * 1e6).round() as i64,
                (p.y * 1e6).round() as i64,
                (p.z * 1e6).round() as i64,
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn test_make_independent_keeps_the_subgroups_and_shares_nothing() {
    let (mut s, mut h) = scene_history();
    let t = &mut s.doc.meshes;
    let mut parent = placed(t, quad(0.), 5.0, 0.0);
    let k1 = placed(t, quad(0.), 0.0, 1.0);
    let k2 = placed(t, quad(0.), 0.0, 2.0);
    parent.adopt(vec![k1, k2]);
    let sibling = copy_group(t, &mut parent, Some(V3(10., 0., 0.)), false);
    let (pid, sid): (GroupId, GroupId) = (parent.id, sibling.id);
    s.doc.groups.extend([parent, sibling]);
    let parent = s.doc.find_group(pid).unwrap().clone();
    let before = points(&s.doc.meshes, &parent);
    let kids_uid: Vec<String> = parent.children.iter().map(|c| c.uid.clone()).collect();

    assert!(h.execute(&mut s, &mut MakeIndependentCommand { group: pid }));
    let parent = s.doc.find_group(pid).unwrap().clone();
    let sibling = s.doc.find_group(sid).unwrap().clone();
    assert_eq!(parent.children.len(), 2);
    assert_eq!(
        parent
            .children
            .iter()
            .map(|c| c.uid.clone())
            .collect::<Vec<_>>(),
        kids_uid
    );
    assert!(parent.xform.is_some());
    assert_eq!(points(&s.doc.meshes, &parent), before);
    assert_ne!(parent.mesh, sibling.mesh);
    for (mine, theirs) in parent.children.iter().zip(&sibling.children) {
        assert_ne!(mine.mesh, theirs.mesh);
    }
    let theirs = points(&s.doc.meshes, &sibling);
    {
        let m = Arc::make_mut(s.doc.meshes.get_mut(&parent.children[0].mesh).unwrap());
        let v = m.verts.ids()[0];
        m.verts[v].position = V3(0., 0., 9.);
    }
    assert_eq!(points(&s.doc.meshes, &sibling), theirs);

    h.undo(&mut s);
    let parent = s.doc.find_group(pid).unwrap();
    let sibling = s.doc.find_group(sid).unwrap();
    assert_eq!(parent.mesh, sibling.mesh);
    assert!(parent
        .children
        .iter()
        .zip(&sibling.children)
        .all(|(a, b)| a.mesh == b.mesh));
}

#[test]
fn test_a_component_without_subgroups_still_bakes_to_a_group() {
    let (mut s, mut h) = scene_history();
    let t = &mut s.doc.meshes;
    let mut a = placed(t, quad(0.), 3.0, 0.0);
    let b = copy_group(t, &mut a, Some(V3(5., 0., 0.)), false);
    let aid = a.id;
    s.doc.groups.extend([a, b]);
    h.execute(&mut s, &mut MakeIndependentCommand { group: aid });
    let a = s.doc.find_group(aid).unwrap();
    assert!(a.xform.is_none() && a.children.is_empty());
}
