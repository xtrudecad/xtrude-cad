// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Document tests — a group made inside a turned group takes the context's
//! axes.

use crate::common;
use common::*;
use xtrude_core::axes::frame_axes;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::groups::MakeGroupCommand;
use xtrude_model::group::{add_mesh, group_frame, world_mesh, Group};
use xtrude_model::history::Command;
use xtrude_model::scene::Scene;

fn turn(deg: f64) -> DMat4 {
    DMat4::from_translation(V3(2., 1., 0.)) * rotation_deg(DVec3::Z, deg)
}

fn inside_turned_group() -> (Scene, Vec<DVec3>, xtrude_core::FaceId) {
    let mut scene = Scene::new();
    let id = add_mesh(&mut scene.doc.meshes, Mesh::new());
    let mut outer = Group::new(id, Some("outer"));
    outer.axes = Some(turn(30.0));
    let gid = outer.id;
    scene.doc.groups.push(outer);
    scene.begin_group_edit(gid);
    let pts = vec![
        V3(2., 1., 0.),
        V3(3., 1., 0.),
        V3(3., 2., 0.),
        V3(2., 2., 0.),
    ];
    let f = scene.mesh_mut().add_face(&pts, &[]);
    (scene, pts, f)
}

fn made(scene: &mut Scene, f: xtrude_core::FaceId, component: bool) -> Group {
    let mut cmd = MakeGroupCommand::new(scene.mesh(), &[f], &[], component, None);
    cmd.apply(scene).unwrap();
    scene.doc.find_group(cmd.group.unwrap()).unwrap().clone()
}

#[test]
fn test_a_group_made_inside_takes_the_contexts_axes() {
    let (mut scene, _pts, f) = inside_turned_group();
    let g = made(&mut scene, f, false);
    let (_o, x, _y, _z) = frame_axes(group_frame(&g).as_ref());
    let a = 30f64.to_radians();
    assert!((x - V3(a.cos(), a.sin(), 0.)).length() < 1e-4);
    xtrude_core::axes::sync(None);
}

#[test]
fn test_a_component_made_inside_is_placed_on_the_contexts_axes() {
    let (mut scene, pts, f) = inside_turned_group();
    let g = made(&mut scene, f, true);
    let (_o, x, _y, _z) = frame_axes(group_frame(&g).as_ref());
    let a = 30f64.to_radians();
    assert!((x - V3(a.cos(), a.sin(), 0.)).length() < 1e-4);
    let key = |p: DVec3| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
    let mut got: Vec<_> = world_mesh(&scene.doc.meshes, &g)
        .verts
        .iter()
        .map(|(_, v)| key(v.position))
        .collect();
    got.sort();
    let mut want: Vec<_> = pts.iter().map(|&p| key(p)).collect();
    want.sort();
    assert_eq!(got, want);
    xtrude_core::axes::sync(None);
}
