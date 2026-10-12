// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A repaint must not be lost when the same group moves. The frame cache is
//! keyed by the scene version and rebuilt whole, so the contract checked is
//! the drawn result: the frame wears the colours the faces have.

use crate::common;

use glam::DVec3;
use xtrude_app::viewport::Viewport;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{MoveGroupCommand, SetFaceColorCommand};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

const TAN: [f64; 3] = [0.78, 0.63, 0.43];
const GRAY: [f64; 3] = [0.67, 0.67, 0.69];

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.)
}

/// How many distinct colours the frame's faces draw.
fn colors(vp: &mut Viewport) -> usize {
    let f = vp.frame_data([200, 200], false);
    let mut seen: Vec<[i32; 3]> = Vec::new();
    for fv in f.scene_faces.data.iter() {
        let c = [0, 1, 2].map(|i| (fv.front[i] * 1000.0).round() as i32);
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen.len()
}

fn strip(vp: &mut Viewport) -> (GroupId, MeshId, FaceId) {
    let mut m = Mesh::new();
    let corner = m.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    let neighbour = m.add_face(&[v(2., 0.), v(4., 0.), v(4., 2.), v(2., 2.)], &[]);
    m.face_mut(corner).attrs.paint.color = Some(TAN);
    m.face_mut(neighbour).attrs.paint.color = Some(GRAY);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("Strip"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    (gid, mid, corner)
}

#[test]
fn test_repaint_survives_a_move_of_the_same_group() {
    let mut vp = common::sized_viewport(200., 200.);
    let (g, mid, corner) = strip(&mut vp);
    assert_eq!(colors(&mut vp), 2);
    vp.run(SetFaceColorCommand {
        faces: vec![(mid, corner)],
        color: Some(GRAY),
    });
    vp.run(MoveGroupCommand {
        group: g,
        delta: DVec3::new(5., 0., 0.),
    });
    assert_eq!(
        colors(&mut vp),
        1,
        "the moved group kept the corner's old colour"
    );
}

#[test]
fn test_repaint_alone_is_picked_up_too() {
    let mut vp = common::sized_viewport(200., 200.);
    let (_g, mid, corner) = strip(&mut vp);
    colors(&mut vp);
    vp.run(SetFaceColorCommand {
        faces: vec![(mid, corner)],
        color: Some(GRAY),
    });
    assert_eq!(colors(&mut vp), 1);
}

#[test]
fn test_the_flag_reaches_nested_placements() {
    // A nested placement's mesh renders as part of its parent: a repaint
    // of it reaches the frame.
    let mut vp = common::sized_viewport(200., 200.);
    let mut deep = Mesh::new();
    let f = deep.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    deep.face_mut(f).attrs.paint.color = Some(TAN);
    let deep_id = add_mesh(&mut vp.scene.doc.meshes, deep);
    let mut child = Group::new(deep_id, Some("Board"));
    child.xform = Some(glam::DMat4::IDENTITY);
    let parent_mesh = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut parent = Group::new(parent_mesh, Some("Spacer"));
    parent.adopt(vec![child]);
    vp.scene.doc.groups.push(parent);
    vp.scene.version += 1;
    let tan_drawn = |vp: &mut Viewport| {
        let fr = vp.frame_data([200, 200], false);
        fr.scene_faces
            .data
            .iter()
            .any(|fv| (fv.front[2] - fv.front[0]).abs() > 0.2) // tan: r ≫ b
    };
    assert!(tan_drawn(&mut vp));
    vp.run(SetFaceColorCommand {
        faces: vec![(deep_id, f)],
        color: Some(GRAY),
    });
    assert!(!tan_drawn(&mut vp), "the nested mesh was missed");
}
