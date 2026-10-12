// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rule, command and explode tests: a container's paint dresses
//! the default faces inside it; a face's own paint wins. The Paint tool tests
//! live in xtrude-app, the bake ones in xtrude-render.

use crate::common;
use common::*;
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::commands::{
    ExplodeGroupCommand, InsertGroupCommand, MakeGroupCommand, MakeNestedGroupCommand,
    MoveGroupCommand, SetGroupMaterialCommand,
};
use xtrude_model::edits::build_add_edges;
use xtrude_model::group::{add_mesh, copy_group, effective_material, world_mesh, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::materials::{effective_attrs, has_own_material};
use xtrude_model::scene::Scene;

fn paint(c: [f64; 3]) -> Paint {
    Paint {
        color: Some(c),
        ..Paint::default()
    }
}

fn red() -> Paint {
    paint([1.0, 0.0, 0.0])
}

#[test]
fn test_effective_attrs_lets_the_faces_own_paint_win() {
    let r = red();
    let e = effective_attrs(&FaceAttrs::default(), Some(&r));
    assert_eq!(e.paint.color, Some([1.0, 0.0, 0.0]));
    assert_eq!(e.back, Some(Back::Mirror)); // both sides
    let own = FaceAttrs {
        paint: paint([0.0, 0.0, 1.0]),
        ..FaceAttrs::default()
    };
    assert_eq!(effective_attrs(&own, Some(&r)), own); // untouched
    let mut op = FaceAttrs::default();
    op.paint.opacity = Some(0.5);
    assert_eq!(effective_attrs(&op, None), op);
    assert!(!has_own_material(&FaceAttrs::default()) && has_own_material(&own));
}

#[test]
fn test_effective_material_reaches_a_nested_piece_through_its_owner() {
    let r = red();
    // a piece with no paint of its own takes its owner's
    assert_eq!(effective_material(None, Some(&r)), Some(&r));
    let green = paint([0., 1., 0.]);
    assert_eq!(effective_material(Some(&green), Some(&r)), Some(&green)); // its own first
    assert_eq!(effective_material(None, None), None);
}

fn two_face_group(scene: &mut Scene) -> (GroupId, FaceId, FaceId) {
    let mut m = Mesh::new();
    let plain = m.add_face(&[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)], &[]);
    let blue = m.add_face(&[V(2., 0.), V(3., 0.), V(3., 1.), V(2., 1.)], &[]);
    m.face_mut(blue).attrs.paint.color = Some([0.0, 0.0, 1.0]);
    let mid = add_mesh(&mut scene.doc.meshes, m);
    let g = Group::new(mid, Some("box"));
    let gid = g.id;
    scene.doc.groups.push(g);
    scene.version += 1;
    (gid, plain, blue)
}

#[test]
fn test_set_group_material_is_one_undo_step_and_touches_no_face() {
    let (mut s, mut h) = scene_history();
    let (g, plain, blue) = two_face_group(&mut s);
    exec(
        &mut s,
        &mut h,
        Box::new(SetGroupMaterialCommand {
            group: g,
            material: Some(red()),
        }),
    );
    let grp = s.doc.find_group(g).unwrap();
    assert_eq!(grp.material, Some(red()));
    let m = s.doc.mesh(grp.mesh);
    assert_eq!(m.face(blue).attrs.paint.color, Some([0.0, 0.0, 1.0]));
    assert!(m.face(plain).attrs.paint.color.is_none()); // still default
    assert_eq!(h.undo_len(), 1);
    assert!(h.undo(&mut s));
    assert!(s.doc.find_group(g).unwrap().material.is_none());
    assert!(h.redo(&mut s));
    assert_eq!(s.doc.find_group(g).unwrap().material, Some(red()));
    let mut grp = s.doc.find_group(g).unwrap().clone();
    assert_eq!(
        copy_group(&mut s.doc.meshes, &mut grp, None, false).material,
        Some(red())
    );
}

fn loose_colours(s: &Scene) -> Vec<[i64; 3]> {
    let m = s.mesh();
    let mut v: Vec<[i64; 3]> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let c = m.face(f).attrs.paint.color.expect("painted");
            [c[0] as i64, c[1] as i64, c[2] as i64]
        })
        .collect();
    v.sort();
    v
}

#[test]
fn test_explode_leaves_the_paint_on_the_faces_that_wore_it() {
    let (mut s, mut h) = scene_history();
    let (g, _, _) = two_face_group(&mut s);
    s.doc.find_group_mut(g).unwrap().material = Some(red());
    exec(&mut s, &mut h, Box::new(ExplodeGroupCommand { group: g }));
    // own kept, the default one took the container's
    assert_eq!(loose_colours(&s), vec![[0, 0, 1], [1, 0, 0]]);
    assert!(h.undo(&mut s));
    assert!(s.mesh().faces.is_empty());
    assert_eq!(s.doc.find_group(g).unwrap().material, Some(red()));
}

fn xs_ys(s: &Scene, g: &Group) -> (Vec<i64>, Vec<i64>) {
    let wm = world_mesh(&s.doc.meshes, g);
    let mut xs: Vec<i64> = wm
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1e6).round() as i64)
        .collect();
    let mut ys: Vec<i64> = wm
        .verts
        .iter()
        .map(|(_, v)| (v.position.y * 1e6).round() as i64)
        .collect();
    xs.sort();
    xs.dedup();
    ys.sort();
    ys.dedup();
    (xs, ys)
}

#[test]
fn test_exploding_a_group_of_groups_lifts_them_whole() {
    let (mut s, mut h) = scene_history();
    let sq = [V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&s, &segs, true, vec![], None);
    exec(&mut s, &mut h, cmd);
    let (faces, edges) = (s.mesh().face_ids(), s.mesh().edge_ids());
    let mk = MakeGroupCommand::new(s.mesh(), &faces, &edges, false, None);
    exec(&mut s, &mut h, Box::new(mk));
    let a = s.doc.groups[0].id;
    let mut ga = s.doc.groups[0].clone();
    let mut gb = copy_group(&mut s.doc.meshes, &mut ga, Some(V(3., 0.)), false);
    gb.material = Some(paint([0.0, 0.0, 1.0])); // painted itself
    let b = gb.id;
    exec(&mut s, &mut h, Box::new(InsertGroupCommand::new(gb)));
    let nest = MakeNestedGroupCommand::new(s.mesh(), &[], &[], vec![a, b], None);
    exec(&mut s, &mut h, Box::new(nest));
    let big = s.doc.groups[0].id;
    exec(
        &mut s,
        &mut h,
        Box::new(MoveGroupCommand {
            group: big,
            delta: V(0., 5.),
        }),
    );
    exec(
        &mut s,
        &mut h,
        Box::new(SetGroupMaterialCommand {
            group: big,
            material: Some(red()),
        }),
    );
    exec(&mut s, &mut h, Box::new(ExplodeGroupCommand { group: big }));
    let ids: Vec<GroupId> = s.doc.groups.iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![a, b]);
    assert!(s.mesh().faces.is_empty()); // nothing went loose
    let ga = s.doc.find_group(a).unwrap().clone();
    let gb = s.doc.find_group(b).unwrap().clone();
    let m6 = |v: &[i64]| v.iter().map(|x| *x as f64 / 1e6).collect::<Vec<f64>>();
    let (ax, ay) = xs_ys(&s, &ga);
    assert_eq!((m6(&ax), m6(&ay)), (vec![0.0, 1.0], vec![5.0, 6.0])); // where the move left it
    let (bx, by) = xs_ys(&s, &gb);
    assert_eq!((m6(&bx), m6(&by)), (vec![3.0, 4.0], vec![5.0, 6.0]));
    assert_eq!(ga.material.unwrap().color, Some([1.0, 0.0, 0.0])); // took the container's
    assert_eq!(gb.material.unwrap().color, Some([0.0, 0.0, 1.0])); // kept its own
    assert!(h.undo(&mut s));
    let ids: Vec<GroupId> = s.doc.groups.iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![big]);
    let kids: Vec<GroupId> = s.doc.groups[0].children.iter().map(|g| g.id).collect();
    assert_eq!(kids, vec![a, b]);
    let ga = s.doc.groups[0].children[0].clone();
    assert!(ga.material.is_none());
    // world_mesh of a nested child is in its owner's frame: the container
    // carries the move, so read the child's own (unmoved) coordinates.
    assert_eq!(m6(&xs_ys(&s, &ga).1), vec![0.0, 1.0]);
}

#[test]
fn test_explode_keeps_the_container_paint_on_the_back_too() {
    let (mut s, mut h) = scene_history();
    let (g, _, _) = two_face_group(&mut s);
    s.doc.find_group_mut(g).unwrap().material = Some(red());
    exec(&mut s, &mut h, Box::new(ExplodeGroupCommand { group: g }));
    let m = s.mesh();
    let red_faces: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face(f).attrs.paint.color == Some([1.0, 0.0, 0.0]))
        .collect();
    assert!(!red_faces.is_empty());
    assert!(red_faces.iter().all(|&f| m.face(f).attrs.back.is_some()));
}
