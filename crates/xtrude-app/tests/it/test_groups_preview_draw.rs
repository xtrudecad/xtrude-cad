// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The groups preview of a Move / Rotate / Paste drag: the moving groups leave
//! the scene buffers once, their own buffers are built once, and every drag
//! frame only changes the matrix they are drawn through, checked through the
//! frame data.

use crate::common;

use glam::{DMat4, DVec3};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};
use xtrude_render::frame::Source;

fn square() -> Mesh {
    let mut m = Mesh::new();
    m.add_face(
        &[
            DVec3::new(0., 0., 0.),
            DVec3::new(1., 0., 0.),
            DVec3::new(1., 1., 0.),
            DVec3::new(0., 1., 0.),
        ],
        &[],
    );
    m
}

#[test]
fn test_a_moving_group_draws_from_its_own_buffers_through_the_preview_matrix() {
    let mut vp = common::sized_viewport(400., 300.);
    let mesh = add_mesh(&mut vp.scene.doc.meshes, square());
    let g = Group::new(mesh, Some("Board"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    let before = vp.frame_data([400, 300], false);
    assert_eq!(before.scene_faces.data.len(), 6); // two triangles
    assert!(before
        .draws
        .iter()
        .all(|d| d.source != Source::PreviewFaces));

    vp.begin_groups_preview(&[gid]);
    vp.set_groups_preview_offset(DVec3::new(5., 0., 0.));
    let f1 = vp.frame_data([400, 300], false);
    assert_eq!(
        f1.scene_faces.data.len(),
        0,
        "the mover left the scene buffers"
    );
    assert_eq!(f1.preview_faces.data.len(), 6);
    let d = f1
        .draws
        .iter()
        .find(|d| d.source == Source::PreviewFaces)
        .expect("a preview draw");
    assert_eq!(
        d.model,
        Some(DMat4::from_translation(DVec3::new(5., 0., 0.)))
    );

    // a drag frame: same buffers (same generation), only the matrix moves
    vp.set_groups_preview_offset(DVec3::new(7., 0., 0.));
    let f2 = vp.frame_data([400, 300], false);
    assert_eq!(f2.preview_faces.generation, f1.preview_faces.generation);
    assert_eq!(f2.scene_faces.generation, f1.scene_faces.generation);
    let d = f2
        .draws
        .iter()
        .find(|d| d.source == Source::PreviewFaces)
        .unwrap();
    assert_eq!(
        d.model,
        Some(DMat4::from_translation(DVec3::new(7., 0., 0.)))
    );

    vp.end_groups_preview();
    let after = vp.frame_data([400, 300], false);
    assert_eq!(after.scene_faces.data.len(), 6);
    assert!(after.draws.iter().all(|d| d.source != Source::PreviewFaces));
}
