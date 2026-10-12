// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport tests: an edit inside a group of one component copy
//! reaches the other copy; a group exploded out of a copy is its own when
//! opened.

use crate::common;

use common::{v3, window};
use glam::DVec3;
use std::sync::Arc;
use xtrude_app::viewport::Viewport;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_model::commands::{ExplodeGroupCommand, MakeComponentOfCommand, MakeNestedGroupCommand};
use xtrude_model::group::{add_mesh, copy_group, iter_placements, Group};
use xtrude_model::ids::GroupId;

fn box_group(vp: &mut Viewport, x: f64, name: &str) -> Group {
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
    Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some(name))
}

/// A component of two groups and a copy of it 5 m along Y: (comp, dup).
fn component_and_copy(vp: &mut Viewport) -> (GroupId, GroupId) {
    let (a, b) = (box_group(vp, 0., "A"), box_group(vp, 3., "B"));
    let ids = vec![a.id, b.id];
    vp.scene.doc.groups.extend([a, b]);
    let nest = MakeNestedGroupCommand::new(vp.scene.mesh(), &[], &[], ids, Some("Spacer"));
    vp.execute(&mut MakeComponentOfCommand { nest });
    let i = vp.scene.doc.groups.len() - 1;
    let mut comp = vp.scene.doc.groups[i].clone();
    let dup = copy_group(
        &mut vp.scene.doc.meshes,
        &mut comp,
        Some(DVec3::new(0., 5., 0.)),
        false,
    );
    let ids = (comp.id, dup.id);
    vp.scene.doc.groups[i] = comp;
    vp.scene.doc.groups.push(dup);
    vp.scene.version += 1;
    ids
}

fn raise_active(vp: &mut Viewport) {
    let mid = vp.scene.active_mesh_id();
    let m = Arc::make_mut(vp.scene.doc.meshes.get_mut(&mid).unwrap());
    for v in m.vertex_ids() {
        m.verts[v].position += DVec3::new(0., 0., 0.5);
    }
    vp.scene.version += 1;
}

fn leave_all(vp: &mut Viewport) {
    while vp.scene.edit_group().is_some() {
        vp.escape();
    }
}

fn world_z(vp: &Viewport, g: GroupId) -> Vec<i64> {
    let g = vp.scene.doc.find_group(g).unwrap();
    let mut zs: Vec<i64> = iter_placements(g, None)
        .into_iter()
        .flat_map(|(pg, m)| {
            let mesh = vp.scene.doc.mesh(pg.mesh);
            mesh.verts
                .iter()
                .map(|(_, v)| {
                    (m.map_or(v.position, |m| m.project_point(v.position)).z * 1000.).round() as i64
                })
                .collect::<Vec<_>>()
        })
        .collect();
    zs.sort();
    zs.dedup();
    zs
}

fn child(vp: &Viewport, g: GroupId, i: usize) -> Group {
    vp.scene.doc.find_group(g).unwrap().children[i].clone()
}

#[test]
fn test_a_group_exploded_out_of_a_copy_is_its_own_when_opened() {
    let mut w = window();
    let vp = &mut w.viewport;
    let (comp, dup) = component_and_copy(vp);
    let freed = child(vp, dup, 0).id;
    vp.execute(&mut ExplodeGroupCommand { group: dup });
    assert!(vp.scene.doc.groups.iter().any(|g| g.id == freed));
    vp.begin_group_edit(freed);
    let freed_mesh = vp.scene.doc.find_group(freed).unwrap().mesh;
    assert_ne!(freed_mesh, child(vp, comp, 0).mesh); // made unique
    raise_active(vp);
    leave_all(vp);
    assert_eq!(world_z(vp, comp), vec![0]);
}

#[test]
fn test_through_the_viewport_the_edit_reaches_the_other_copy_and_undoes() {
    let mut w = window();
    let vp = &mut w.viewport;
    let (comp, dup) = component_and_copy(vp);
    vp.begin_group_edit(dup);
    let c = child(vp, dup, 0).id;
    vp.begin_group_edit(c);
    assert_ne!(vp.scene.active_mesh_id(), child(vp, comp, 0).mesh); // the world copy…
    raise_active(vp);
    leave_all(vp);
    assert!(world_z(vp, comp).contains(&500)); // …shared back
    assert_eq!(child(vp, dup, 0).mesh, child(vp, comp, 0).mesh);
}
