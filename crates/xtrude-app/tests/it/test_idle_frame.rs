// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An idle window does not render the scene again: `changed_frame` hands
//! out a frame only when it differs from the one on screen.

use crate::common;

use glam::DVec3;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::SelectMode;

const SIZE: [u32; 2] = [400, 300];

/// A viewport with a box drawn once, so the next frame is the one on
/// screen.
fn shown_box() -> Viewport {
    let mut vp = common::sized_viewport(400., 300.);
    common::add_box(&mut vp, DVec3::ZERO, DVec3::ONE);
    vp.camera.fit_to(DVec3::ZERO, DVec3::ONE, 1.2);
    assert!(vp.changed_frame(SIZE).is_some(), "the first frame draws");
    vp
}

fn some_face(vp: &Viewport) -> EntityRef {
    let mid = vp.scene.active_mesh_id();
    let f = vp.scene.mesh().face_ids()[0];
    EntityRef::Face(mid, f)
}

#[test]
fn test_no_frame_is_needed_when_nothing_happened() {
    let mut vp = shown_box();
    assert!(vp.changed_frame(SIZE).is_none());
    assert!(vp.changed_frame(SIZE).is_none(), "nor later");
}

#[test]
fn test_a_camera_change_needs_a_frame() {
    let mut vp = shown_box();
    vp.camera.orbit(20.0, 0.0, 300);
    assert!(vp.changed_frame(SIZE).is_some());
    assert!(vp.changed_frame(SIZE).is_none(), "drawn once");
}

#[test]
fn test_a_scene_change_needs_a_frame() {
    let mut vp = shown_box();
    common::add_box(&mut vp, DVec3::new(2., 0., 0.), DVec3::new(3., 1., 1.));
    assert!(vp.changed_frame(SIZE).is_some());
    assert!(vp.changed_frame(SIZE).is_none());
}

#[test]
fn test_a_selection_change_needs_a_frame() {
    let mut vp = shown_box();
    let face = some_face(&vp);
    vp.scene.select([face], SelectMode::Replace);
    assert!(vp.changed_frame(SIZE).is_some());
    assert!(vp.changed_frame(SIZE).is_none());
}

#[test]
fn test_a_hover_needs_a_frame_only_when_the_hovered_entity_changes() {
    let mut vp = shown_box();
    let face = some_face(&vp);
    vp.set_hover(Some(face));
    assert!(vp.changed_frame(SIZE).is_some(), "a new hovered face");
    vp.set_hover(Some(face));
    assert!(vp.changed_frame(SIZE).is_none(), "the same face");
    vp.set_hover(None);
    assert!(vp.changed_frame(SIZE).is_some(), "the hover left it");
}

#[test]
fn test_a_tool_preview_needs_a_frame_at_each_step() {
    let mut vp = shown_box();
    let mut square = Mesh::new();
    square.add_face(
        &[
            DVec3::new(0., 0., 2.),
            DVec3::new(1., 0., 2.),
            DVec3::new(1., 1., 2.),
            DVec3::new(0., 1., 2.),
        ],
        &[],
    );
    let mesh = add_mesh(&mut vp.scene.doc.meshes, square);
    let g = Group::new(mesh, Some("Board"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    assert!(vp.changed_frame(SIZE).is_some());

    vp.begin_groups_preview(&[gid]);
    vp.set_groups_preview_offset(DVec3::new(1., 0., 0.));
    assert!(vp.changed_frame(SIZE).is_some(), "the preview starts");
    assert!(vp.changed_frame(SIZE).is_none());
    vp.set_groups_preview_offset(DVec3::new(2., 0., 0.));
    assert!(vp.changed_frame(SIZE).is_some(), "the preview moves");
    vp.end_groups_preview();
    assert!(vp.changed_frame(SIZE).is_some(), "the preview ends");
}

#[test]
fn test_a_new_size_or_a_new_renderer_needs_a_frame() {
    let mut vp = shown_box();
    assert!(vp.changed_frame([401, 300]).is_some());
    assert!(vp.changed_frame([401, 300]).is_none());
    vp.forget_frame_on_screen();
    assert!(vp.changed_frame([401, 300]).is_some());
}

#[test]
fn test_a_kept_frame_keeps_the_depth_read_from_it() {
    let mut vp = shown_box();
    vp.set_depth_snap(vec![0.5; 400 * 300], 400, 300);
    assert!(vp.changed_frame(SIZE).is_none());
    assert!(vp.depth_snap().is_some(), "the screen still shows it");
    vp.camera.orbit(20.0, 0.0, 300);
    assert!(vp.changed_frame(SIZE).is_some());
    assert!(vp.depth_snap().is_none(), "a new frame drops it");
}
