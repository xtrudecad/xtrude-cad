// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Scene tests: the viewport ones are in xtrude-app.

use crate::common;
use common::*;
use std::sync::Arc;
use xtrude_core::math::MatExt;
use xtrude_core::{DVec3, Mesh};
use xtrude_io::igz;
use xtrude_model::commands::{MakeComponentOfCommand, MakeNestedGroupCommand};
use xtrude_model::group::{add_mesh, copy_group, iter_placements, Group};
use xtrude_model::history::History;
use xtrude_model::ids::GroupId;
use xtrude_model::scene::Scene;

fn box_group(s: &mut Scene, x: f64, name: &str) -> Group {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ],
        &[],
    );
    Group::new(add_mesh(&mut s.doc.meshes, m), Some(name))
}

fn component_and_copy() -> Scene {
    copy_of(true)
}

/// A container of two groups — a component, or a plain group of groups —
/// and a copy of it 5 m along Y.
fn copy_of(component: bool) -> Scene {
    let mut s = Scene::new();
    let mut h = History::new();
    let (a, b) = (box_group(&mut s, 0., "A"), box_group(&mut s, 3., "B"));
    let ids: Vec<GroupId> = vec![a.id, b.id];
    s.doc.groups.extend([a, b]);
    let nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], ids, Some("Spacer"));
    if component {
        h.execute(&mut s, &mut MakeComponentOfCommand { nest });
    } else {
        let mut nest = nest;
        h.execute(&mut s, &mut nest);
    }
    let mut comp = s.doc.groups[0].clone();
    let dup = copy_group(
        &mut s.doc.meshes,
        &mut comp,
        Some(DVec3::new(0., 5., 0.)),
        false,
    );
    s.doc.groups[0] = comp;
    s.doc.groups.push(dup);
    s
}

fn raise_first_child(s: &mut Scene, container: GroupId) {
    s.begin_group_edit(container);
    let child = s.doc.find_group(container).unwrap().children[0].id;
    s.begin_group_edit(child);
    let mid = s.active_mesh_id();
    let m = Arc::make_mut(s.doc.meshes.get_mut(&mid).unwrap());
    let mut seen = std::collections::HashSet::new();
    for f in m.face_ids() {
        for v in m.face(f).outer.clone() {
            if seen.insert(v) {
                m.verts[v].position += DVec3::new(0., 0., 0.5);
            }
        }
    }
    s.end_one_group_edit();
    s.end_one_group_edit();
}

fn world_z(s: &Scene, g: &Group) -> Vec<i64> {
    let mut zs: Vec<i64> = iter_placements(g, None)
        .into_iter()
        .flat_map(|(pg, m)| {
            let mesh = s.doc.mesh(pg.mesh);
            mesh.face_ids()
                .into_iter()
                .flat_map(|f| mesh.face_positions(f))
                .map(|p| m.map_or(p, |m| m.project_point(p)).z)
                .map(|z| (z * 1000.0).round() as i64)
                .collect::<Vec<_>>()
        })
        .collect();
    zs.sort();
    zs.dedup();
    zs
}

#[test]
fn test_the_sharing_survives_the_igz() {
    let s = component_and_copy();
    let tmp = TmpDir::new();
    let path = tmp.join("spacer.igz");
    igz::save_scene(&s, &path).unwrap();
    let mut loaded = Scene::new();
    igz::load_into(&mut loaded, &path, None).unwrap();
    let (c1, c2) = (loaded.doc.groups[0].clone(), loaded.doc.groups[1].clone());
    assert!(c1
        .children
        .iter()
        .zip(&c2.children)
        .all(|(x, y)| x.mesh == y.mesh));
    raise_first_child(&mut loaded, c2.id);
    let c1 = loaded.doc.groups[0].clone();
    assert!(world_z(&loaded, &c1).contains(&500));
}

#[test]
fn test_editing_a_group_inside_one_copy_changes_every_copy() {
    let mut s = component_and_copy();
    let dup = s.doc.groups[1].id;
    raise_first_child(&mut s, dup);
    let comp = s.doc.groups[0].clone();
    assert!(world_z(&s, &comp).contains(&500)); // the original copy follows
}

#[test]
fn test_copies_of_a_group_of_groups_stay_independent() {
    let mut s = copy_of(false);
    let dup = s.doc.groups[1].id;
    raise_first_child(&mut s, dup);
    let grp = s.doc.groups[0].clone();
    assert_eq!(world_z(&s, &grp), vec![0]);
}
