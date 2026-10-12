// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! With a face selected the right-click menu offers Reverse Faces (edges alone
//! do not), and the entry does the work, in one undo step.

use crate::common;

use common::{context_texts, v3, window};
use xtrude_app::window::MainWindow;
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

fn loose_face(w: &mut MainWindow) -> FaceId {
    w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    )
}

fn entries(w: &mut MainWindow, sel: Vec<EntityRef>) -> Vec<String> {
    w.viewport.scene.select(sel, SelectMode::Replace);
    context_texts(w)
}

#[test]
fn test_with_a_face_selected_the_menu_offers_reverse() {
    let mut w = window();
    let f = loose_face(&mut w);
    assert!(entries(&mut w, vec![EntityRef::Face(MeshId::LOOSE, f)])
        .contains(&"Reverse Faces".to_string()));
}

#[test]
fn test_with_only_edges_it_does_not_show() {
    let mut w = window();
    loose_face(&mut w);
    let e = w.viewport.scene.mesh().edge_ids()[0];
    let texts = entries(&mut w, vec![EntityRef::Edge(MeshId::LOOSE, e)]);
    assert!(!texts.contains(&"Reverse Faces".to_string()));
    // «Hide»: the one entry hides objects and edges alike.
    assert!(texts.contains(&"Hide".to_string()), "{texts:?}");
}

#[test]
fn test_reverse_from_the_menu_flips_the_face() {
    let mut w = window();
    let f = loose_face(&mut w);
    let before = w.viewport.scene.mesh().face_normal(f);
    w.viewport
        .scene
        .select([EntityRef::Face(MeshId::LOOSE, f)], SelectMode::Replace);
    w.trigger("reverse_faces");
    assert!(w.viewport.scene.mesh().face_normal(f).dot(before) < -0.99);
    w.viewport.undo();
    assert!(w.viewport.scene.mesh().face_normal(f).dot(before) > 0.99);
}
