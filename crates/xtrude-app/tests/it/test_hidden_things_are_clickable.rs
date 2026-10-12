// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hidden objects and geometry shown as ghosts take the click (the pick
//! index keys on the two view switches too); a click never
//! lands on a seam it cannot see, nor on an edge behind the model.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{sized_viewport, v3};
use glam::DMat4;
use xtrude_app::tools::select;
use xtrude_app::viewport::Viewport;
use xtrude_core::{EdgeId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn vp() -> Viewport {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v3(3., 3., 0.);
    vp.camera.distance = 12.0;
    vp
}

fn boxed_group(vp: &mut Viewport) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut g = Group::new(mid, Some("box"));
    g.xform = Some(DMat4::from_translation(v3(1., 1., 0.)));
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    id
}

fn edge_id(vp: &mut Viewport, x: f64, y: f64, visible: bool) -> Option<EdgeId> {
    let e = if visible {
        vp.pick_visible_edge(x, y)
    } else {
        vp.pick_edge(x, y)
    };
    e.and_then(|e| e.edge).map(|(_, id)| id)
}

#[test]
fn test_a_hidden_object_shown_as_a_ghost_takes_the_click() {
    let mut vp = vp();
    let g = boxed_group(&mut vp);
    let ScreenPt { x, y } = vp.world_to_pixel(v3(2., 2., 0.)).unwrap();
    assert_eq!(vp.pick_group(x, y), Some(g));
    vp.scene
        .doc
        .groups
        .iter_mut()
        .find(|q| q.id == g)
        .unwrap()
        .hidden = true;
    vp.scene.version += 1;
    assert_eq!(vp.pick_group(x, y), None); // hidden: gone
    vp.scene.show_hidden_objects = true; // View ▸ Hidden Groups
    vp.scene.version += 1;
    assert_eq!(vp.pick_group(x, y), Some(g)); // the ghost is clickable
    vp.scene.show_hidden_objects = false;
    vp.scene.version += 1;
    assert_eq!(vp.pick_group(x, y), None);
}

#[test]
fn test_a_hidden_face_shown_as_a_ghost_takes_the_click() {
    let mut vp = vp();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    vp.scene.version += 1;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(1., 1., 0.)).unwrap();
    assert_eq!(vp.pick_face(x, y), Some((MeshId::LOOSE, f)));
    vp.scene.mesh_mut().face_mut(f).attrs.hidden = true;
    vp.scene.version += 1;
    assert_eq!(vp.pick_face(x, y), None);
    vp.scene.show_hidden_geometry = true;
    vp.scene.version += 1;
    assert_eq!(vp.pick_face(x, y), Some((MeshId::LOOSE, f)));
}

#[test]
fn test_a_hidden_edge_is_clickable_only_while_shown() {
    let mut vp = vp();
    let e = vp
        .scene
        .mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(4., 0., 0.))
        .unwrap();
    vp.scene.version += 1;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(2., 0., 0.)).unwrap();
    assert_eq!(edge_id(&mut vp, x, y, false), Some(e));
    vp.scene.mesh_mut().edge_mut(e).hidden = true;
    vp.scene.version += 1;
    assert_eq!(edge_id(&mut vp, x, y, false), None); // as window selection
    vp.scene.show_hidden_geometry = true;
    vp.scene.version += 1;
    assert_eq!(edge_id(&mut vp, x, y, false), Some(e));
}

// ---- cylinder faces are easy to select ------------------------------------------

/// A 2×2 square split by a SOFT diagonal, plus a hard edge lying under it
/// on the floor, 1 m down.
fn box_with_soft_diagonal(vp: &mut Viewport) -> (EdgeId, EdgeId) {
    let (a, b, c, d) = (
        v3(0., 0., 1.),
        v3(2., 0., 1.),
        v3(2., 2., 1.),
        v3(0., 2., 1.),
    );
    let m = vp.scene.mesh_mut();
    m.add_face(&[a, b, c], &[]);
    m.add_face(&[a, c, d], &[]);
    let seam = m
        .find_edge(m.vertex_at(a).unwrap(), m.vertex_at(c).unwrap())
        .unwrap();
    m.edge_mut(seam).soft = true;
    let below = m.add_edge(v3(-1., 1., 0.), v3(3., 1., 0.)).unwrap();
    vp.scene.version += 1;
    (seam, below)
}

#[test]
fn test_a_click_never_lands_on_a_seam_it_cannot_see() {
    let mut vp = vp();
    let (seam, _below) = box_with_soft_diagonal(&mut vp);
    let ScreenPt { x, y } = vp.world_to_pixel(v3(1., 1., 1.)).unwrap();
    assert_eq!(edge_id(&mut vp, x, y, false), Some(seam)); // references see it
    assert_eq!(edge_id(&mut vp, x, y, true), None); // a click does not
    vp.scene.show_hidden_geometry = true;
    vp.scene.version += 1;
    assert_eq!(edge_id(&mut vp, x, y, true), Some(seam)); // shown: clickable
}

#[test]
fn test_a_click_never_lands_on_an_edge_behind_the_model() {
    let mut vp = vp();
    let (_seam, below) = box_with_soft_diagonal(&mut vp);
    let ScreenPt { x, y } = vp.world_to_pixel(v3(1.6, 1., 0.)).unwrap(); // through the lid
    assert_eq!(edge_id(&mut vp, x, y, false), Some(below)); // nearest in 2D
    assert_eq!(edge_id(&mut vp, x, y, true), None); // but under the lid
    let ScreenPt { x, y } = vp.world_to_pixel(v3(-0.5, 1., 0.)).unwrap(); // beside the box
    assert_eq!(edge_id(&mut vp, x, y, true), Some(below));
}

#[test]
fn test_select_takes_the_face_of_a_smooth_side() {
    let mut vp = vp();
    box_with_soft_diagonal(&mut vp);
    let ScreenPt { x, y } = vp.world_to_pixel(v3(1.2, 1.0, 1.)).unwrap();
    let picked = select::pick(&mut vp, x, y);
    assert!(
        matches!(picked, Some(EntityRef::Face(MeshId::LOOSE, f)) if vp.scene.mesh().has_face(f))
    );
}
