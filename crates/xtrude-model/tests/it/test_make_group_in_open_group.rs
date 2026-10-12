// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Make Group inside an open group makes the new group a child of the open
//! group, with the geometry where it was; inside a component copy, every
//! copy gets it.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::{MakeGroupCommand, MakeNestedGroupCommand};
use xtrude_model::group::{add_mesh, world_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::{History, Scene};

/// A unit square at `x`, height `z`.
fn square(m: &mut Mesh, x: f64, z: f64) {
    m.add_face(
        &[
            V3(x, 0., z),
            V3(x + 1., 0., z),
            V3(x + 1., 1., z),
            V3(x, 1., z),
        ],
        &[],
    );
}

/// Three squares side by side, at `x` = 0, 2 and 4.
fn three_squares() -> Mesh {
    let mut m = Mesh::new();
    for x in [0., 2., 4.] {
        square(&mut m, x, 0.);
    }
    m
}

fn g(s: &Scene, id: GroupId) -> &Group {
    s.doc.find_group(id).unwrap()
}

/// The open group's faces whose corners all pass `near` (in world
/// coordinates).
fn faces_where(s: &Scene, near: impl Fn(DVec3) -> bool) -> Vec<xtrude_core::FaceId> {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).into_iter().all(&near))
        .collect()
}

/// The first two squares of an open group placed without a turn.
fn first_two_faces(s: &Scene) -> Vec<xtrude_core::FaceId> {
    faces_where(s, |p| p.x < 3.5)
}

/// Make Group on the open group's first two squares; the new group.
fn group_first_two(s: &mut Scene, h: &mut History) -> GroupId {
    let faces = first_two_faces(s);
    group_faces(s, h, faces)
}

fn group_faces(s: &mut Scene, h: &mut History, faces: Vec<xtrude_core::FaceId>) -> GroupId {
    assert_eq!(faces.len(), 2);
    let mut mk = MakeGroupCommand::new(s.mesh(), &faces, &[], false, Some("Pair"));
    assert!(h.execute(s, &mut mk));
    mk.group.unwrap()
}

fn sorted_world_xs(m: &Mesh) -> Vec<i64> {
    let mut v: Vec<i64> = m
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1000.).round() as i64)
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn test_make_group_in_a_classic_group_nests_it() {
    let (mut s, mut h) = scene_history();
    let mid = add_mesh(&mut s.doc.meshes, three_squares());
    let outer = Group::new(mid, Some("Outer"));
    let oid = outer.id;
    s.doc.groups.push(outer);
    s.begin_group_edit(oid);
    let pair = group_first_two(&mut s, &mut h);
    assert_eq!(s.doc.groups.len(), 1, "nothing new at the top level");
    let o = g(&s, oid);
    assert_eq!(o.children.len(), 1);
    assert_eq!(o.children[0].id, pair);
    assert!(
        o.children[0].xform.is_none(),
        "a plain group, as at the top level"
    );
    assert_eq!(s.doc.mesh(o.mesh).faces.len(), 1, "one square stays");
    let made = world_mesh(&s.doc.meshes, g(&s, pair));
    assert_eq!(made.faces.len(), 2);
    assert_eq!(
        sorted_world_xs(&made),
        [0, 1000, 2000, 3000],
        "where it was"
    );
    assert_eq!(s.edit_group(), Some(oid), "the group stays open");
    assert!(h.undo(&mut s));
    assert!(g(&s, oid).children.is_empty());
    assert_eq!(s.doc.mesh(g(&s, oid).mesh).faces.len(), 3);
    assert_eq!(s.doc.groups.len(), 1);
}

#[test]
fn test_make_group_at_the_top_level_is_unchanged() {
    let (mut s, mut h) = scene_history();
    *s.mesh_mut() = three_squares();
    let pair = group_first_two(&mut s, &mut h);
    assert_eq!(s.doc.groups.len(), 1);
    assert_eq!(s.doc.groups[0].id, pair);
    assert!(s.doc.groups[0].xform.is_none());
    assert_eq!(s.loose_mesh().faces.len(), 1);
}

/// One component without children placed three times, the third turned a
/// quarter round about Z at x = 10: (copies, their shared mesh).
fn three_copies(s: &mut Scene) -> ([GroupId; 3], MeshId) {
    let mid = add_mesh(&mut s.doc.meshes, three_squares());
    let turned =
        DMat4::from_translation(V3(10., 0., 0.)) * DMat4::from_rotation_z(90f64.to_radians());
    let places = [
        DMat4::IDENTITY,
        DMat4::from_translation(V3(0., 5., 0.)),
        turned,
    ];
    let ids = places.map(|x| {
        let mut c = Group::new(mid, Some("Plate"));
        c.xform = Some(x);
        c.component = true;
        let id = c.id;
        s.doc.groups.push(c);
        id
    });
    (ids, mid)
}

#[test]
fn test_make_group_in_a_component_copy_reaches_every_copy() {
    let (mut s, mut h) = scene_history();
    let (copies, mid) = three_copies(&mut s);
    let before = world_mesh(&s.doc.meshes, g(&s, copies[2]));
    s.begin_group_edit(copies[2]);
    // Turned a quarter round: local x runs along world y.
    let faces = faces_where(&s, |p| p.y < 3.5);
    group_faces(&mut s, &mut h, faces);
    let pair = &g(&s, copies[2]).children[0];
    assert!(
        pair.xform.is_some() && !pair.component,
        "placed in the definition"
    );
    s.end_group_edit();
    assert_eq!(s.doc.groups.len(), 3, "nothing new at the top level");
    let pairs: Vec<&Group> = copies.iter().map(|&c| &g(&s, c).children[0]).collect();
    let ids: std::collections::HashSet<GroupId> = pairs.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 3, "each copy has its own GroupId");
    assert!(pairs.iter().all(|p| p.mesh == pairs[0].mesh), "one mesh");
    for p in &pairs[1..] {
        assert!(
            p.xform.unwrap().abs_diff_eq(pairs[0].xform.unwrap(), 1e-9),
            "same local place"
        );
    }
    assert!(
        copies.iter().all(|&c| g(&s, c).mesh == mid),
        "still one definition"
    );
    assert_eq!(s.doc.mesh(mid).faces.len(), 1);
    // In the turned copy, the geometry is where it was.
    let after = world_mesh(&s.doc.meshes, g(&s, copies[2]));
    let mut a: Vec<DVec3> = before.verts.iter().map(|(_, v)| v.position).collect();
    let mut b: Vec<DVec3> = after.verts.iter().map(|(_, v)| v.position).collect();
    let key = |p: &DVec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
    a.sort_by_key(key);
    b.sort_by_key(key);
    assert_eq!(a.len(), b.len());
    assert!(
        a.iter().zip(&b).all(|(p, q)| (*p - *q).length() < 1e-9),
        "{a:?} {b:?}"
    );
    // In the first copy, at its own place.
    let first = world_mesh(&s.doc.meshes, g(&s, copies[0]));
    assert_eq!(sorted_world_xs(&first), [0, 1000, 2000, 3000, 4000, 5000]);
}

#[test]
fn test_nesting_inside_an_open_group_makes_a_child_container() {
    let (mut s, mut h) = scene_history();
    let mid = add_mesh(&mut s.doc.meshes, three_squares());
    let mut bolt_mesh = Mesh::new();
    square(&mut bolt_mesh, 0., 2.);
    let bolt = Group::new(add_mesh(&mut s.doc.meshes, bolt_mesh), Some("Bolt"));
    let bid = bolt.id;
    let mut outer = Group::new(mid, Some("Outer"));
    outer.adopt(vec![bolt]);
    let oid = outer.id;
    s.doc.groups.push(outer);
    s.begin_group_edit(oid);
    let faces = first_two_faces(&s);
    let mut mk = MakeNestedGroupCommand::new(s.mesh(), &faces, &[], vec![bid], None);
    assert!(h.execute(&mut s, &mut mk));
    let cid = mk.container.unwrap();
    assert_eq!(s.doc.groups.len(), 1, "nothing new at the top level");
    let o = g(&s, oid);
    assert_eq!(o.children.len(), 1, "the container replaces the bolt");
    assert_eq!(o.children[0].id, cid);
    let c = g(&s, cid);
    assert_eq!(c.children.len(), 1);
    assert_eq!(c.children[0].id, bid, "the bolt is in the container");
    assert_eq!(s.doc.mesh(c.mesh).faces.len(), 2, "with the two squares");
    let bolt_world = world_mesh(&s.doc.meshes, g(&s, bid));
    assert!(bolt_world
        .verts
        .iter()
        .all(|(_, v)| (v.position.z - 2.).abs() < 1e-9));
    assert!(h.undo(&mut s));
    let o = g(&s, oid);
    assert_eq!(o.children.len(), 1);
    assert_eq!(o.children[0].id, bid);
}
