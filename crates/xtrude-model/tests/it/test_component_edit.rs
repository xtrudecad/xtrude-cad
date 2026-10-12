// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Entering an instance edits a world copy; leaving shares it back to every
//! sibling.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::ReshareInstanceCommand;
use xtrude_model::group::{add_mesh, world_mesh, Group};
use xtrude_model::history::FnCommand;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::Scene;

fn square(m: &mut Mesh, z: f64) {
    m.add_face(
        &[V3(0., 0., z), V3(1., 0., z), V3(1., 1., z), V3(0., 1., z)],
        &[],
    );
}

fn two_instances() -> (Scene, MeshId, GroupId, GroupId) {
    let mut s = Scene::new();
    let mut proto = Mesh::new();
    square(&mut proto, 0.0);
    let pid = add_mesh(&mut s.doc.meshes, proto);
    let mut a = Group::new(pid, Some("a"));
    let mut b = Group::new(pid, Some("b"));
    a.xform = Some(DMat4::IDENTITY);
    b.xform = Some(DMat4::from_translation(V3(10., 0., 0.)));
    let (aid, bid) = (a.id, b.id);
    s.doc.groups.extend([a, b]);
    (s, pid, aid, bid)
}

fn g(s: &Scene, id: GroupId) -> &Group {
    s.doc.find_group(id).unwrap()
}

fn rounded(vals: impl Iterator<Item = f64>) -> Vec<i64> {
    let mut v: Vec<i64> = vals.map(|x| (x * 1000.0).round() as i64).collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn test_entering_edits_a_world_copy_and_leaving_shares_it_back() {
    let (mut s, pid, aid, bid) = two_instances();
    s.begin_group_edit(bid);
    assert_eq!(s.active_mesh_id(), g(&s, bid).mesh);
    assert!(g(&s, bid).mesh != pid && g(&s, bid).xform.is_none());
    assert_eq!(
        rounded(s.mesh().verts.iter().map(|(_, v)| v.position.x)),
        [10000, 11000]
    );
    assert_eq!(s.doc.mesh(pid).faces.len(), 1);
    s.mesh_mut().add_face(
        &[
            V3(10., 0., 1.),
            V3(11., 0., 1.),
            V3(11., 1., 1.),
            V3(10., 1., 1.),
        ],
        &[],
    );
    s.end_group_edit();
    assert!(g(&s, bid).mesh == pid && g(&s, bid).xform.is_some());
    let proto = s.doc.mesh(pid);
    assert_eq!(proto.faces.len(), 2);
    assert_eq!(
        rounded(proto.verts.iter().map(|(_, v)| v.position.z)),
        [0, 1000]
    );
    assert_eq!(
        rounded(proto.verts.iter().map(|(_, v)| v.position.x)),
        [0, 1000]
    );
    assert_eq!(world_mesh(&s.doc.meshes, g(&s, aid)).faces.len(), 2);
    let max_x = world_mesh(&s.doc.meshes, g(&s, bid))
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MIN, f64::max);
    assert!((max_x - 11.0).abs() < 1e-12);
}

#[test]
fn test_looking_without_editing_keeps_the_sharing_silently() {
    let (mut s, pid, _aid, bid) = two_instances();
    s.begin_group_edit(bid);
    s.end_group_edit();
    assert!(
        g(&s, bid).mesh == pid && g(&s, bid).xform.is_some() && s.doc.mesh(pid).faces.len() == 1
    );
}

fn add_square_at(z: f64) -> impl FnMut(&mut Scene) -> xtrude_model::history::CmdResult {
    move |s: &mut Scene| {
        s.mesh_mut().add_face(
            &[
                V3(10., 0., z),
                V3(11., 0., z),
                V3(11., 1., z),
                V3(10., 1., z),
            ],
            &[],
        );
        Ok(())
    }
}

#[test]
fn test_reshare_command_undoes_the_whole_session_in_one_step() {
    let (mut s, pid, aid, bid) = two_instances();
    let mut h = xtrude_model::history::History::new();
    s.begin_group_edit(bid);
    let mark = h.undo_len();
    h.execute(
        &mut s,
        &mut FnCommand::new("SnapshotMutation", add_square_at(1.0)),
    );
    h.execute(
        &mut s,
        &mut FnCommand::new("SnapshotMutation", add_square_at(2.0)),
    );
    assert_eq!(nfaces(&s), 3);
    let share = s.take_edit_share().unwrap();
    assert!(share.group == bid && share.proto == pid);
    let edited = g(&s, bid).mesh;
    s.end_group_edit();
    let inner = h.undo_len() - mark;
    h.execute(
        &mut s,
        &mut ReshareInstanceCommand {
            group: bid,
            proto: pid,
            xform: share.xform,
            edited,
        },
    );
    let xform = share.xform;
    h.squash_last_with(inner + 1, "ReshareInstanceCommand", |before| {
        if let Some(b) = before.find_group_mut(bid) {
            b.mesh = pid;
            b.xform = Some(xform);
        }
    });
    assert!(
        s.doc.mesh(pid).faces.len() == 3 && g(&s, bid).mesh == pid && g(&s, bid).xform.is_some()
    );
    assert_eq!(h.undo_len(), 1);
    assert!(h.undo(&mut s));
    assert!(
        s.doc.mesh(pid).faces.len() == 1 && g(&s, bid).mesh == pid && g(&s, bid).xform.is_some()
    );
    assert_eq!(world_mesh(&s.doc.meshes, g(&s, aid)).faces.len(), 1);
    assert!(h.redo(&mut s));
    assert_eq!(s.doc.mesh(pid).faces.len(), 3);
    assert_eq!(world_mesh(&s.doc.meshes, g(&s, aid)).faces.len(), 3);
}

#[test]
fn test_a_group_with_nested_placements_keeps_them() {
    let (mut s, pid, aid, bid) = two_instances();
    let mut cm = Mesh::new();
    square(&mut cm, 5.0);
    let mut child = Group::new(add_mesh(&mut s.doc.meshes, cm), Some("child"));
    child.xform = Some(DMat4::IDENTITY);
    let child_id = child.id;
    s.doc.find_group_mut(bid).unwrap().adopt(vec![child]);
    s.begin_group_edit(bid);
    let share = s.edit_stack.last().unwrap().share.as_ref();
    assert!(
        share.is_some_and(|sh| sh.contents.is_some()),
        "a container copy shares back its contents",
    );
    let kids: Vec<GroupId> = g(&s, bid).children.iter().map(|k| k.id).collect();
    assert_eq!(kids, [child_id], "the children were baked on entry");
    assert_eq!(s.doc.mesh(g(&s, child_id).mesh).faces.len(), 1);
    s.end_group_edit();
    let kids: Vec<GroupId> = g(&s, bid).children.iter().map(|k| k.id).collect();
    assert_eq!(kids, [child_id]);
    assert!(g(&s, aid).mesh == pid && s.doc.mesh(pid).faces.len() == 1);
    let _ = DVec3::ZERO;
}

#[test]
fn test_leaving_a_container_copy_shares_its_faces_and_its_matrix_comes_back() {
    let (mut s, pid, aid, bid) = two_instances();
    let mut cm = Mesh::new();
    square(&mut cm, 5.0);
    let cid = add_mesh(&mut s.doc.meshes, cm);
    for id in [aid, bid] {
        let mut child = Group::new(cid, Some("child"));
        child.xform = Some(DMat4::from_translation(V3(0., 2., 0.)));
        s.doc.find_group_mut(id).unwrap().adopt(vec![child]);
    }
    let (xb, kids) = (g(&s, bid).xform, g(&s, bid).children.clone());
    s.begin_group_edit(bid);
    assert_eq!(g(&s, bid).xform, Some(DMat4::IDENTITY), "open: at identity");
    add_square_at(1.0)(&mut s).unwrap();
    s.end_group_edit();
    assert!(g(&s, aid).mesh == pid && g(&s, bid).mesh == pid);
    assert_eq!(s.doc.mesh(pid).faces.len(), 2, "both copies show the face");
    assert_eq!(
        rounded(s.doc.mesh(pid).verts.iter().map(|(_, v)| v.position.x)),
        [0, 1000],
        "drawn at x 10..11 in copy b: x 0..1 in the definition"
    );
    assert_eq!(g(&s, bid).xform, xb);
    assert_eq!(g(&s, bid).children, kids);
}
