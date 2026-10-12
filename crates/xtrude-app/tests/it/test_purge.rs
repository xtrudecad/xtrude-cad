// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Layers-panel tests: deleting a layer reassigns the geometry inside groups
//! too, and the Purge button sweeps the unused layers as one step Ctrl+Z
//! brings back. (The survey and command tests are in xtrude-model.)

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{v3, window};
use xtrude_app::tray::Tray;
use xtrude_app::window::{MainWindow, ScriptedDialogs};
use xtrude_core::math::DMat4;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{assign_layer, layer_of};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::layers::{Layer, DEFAULT_LAYER};

fn quad(m: &mut Mesh) -> FaceId {
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    )
}

fn names(w: &MainWindow) -> Vec<String> {
    w.viewport
        .scene
        .doc
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_deleting_a_layer_reassigns_faces_INSIDE_groups() {
    // The − button walked the top-level groups only, so geometry inside a
    // group kept a tag pointing at a layer that no longer existed.
    let mut w = window();
    let mut tray = Tray::default();
    let scene = &mut w.viewport.scene;
    let mut inner = Mesh::new();
    let f = quad(&mut inner);
    let mut deep = Mesh::new();
    let g = quad(&mut deep);
    let (im, dm) = (
        add_mesh(&mut scene.doc.meshes, inner),
        add_mesh(&mut scene.doc.meshes, deep),
    );
    let mut child = Group::new(dm, Some("Clip"));
    child.xform = Some(DMat4::IDENTITY);
    let mut parent = Group::new(im, Some("Mesa"));
    parent.adopt(vec![child]);
    scene.doc.groups.push(parent);
    scene.doc.layers.push(Layer::new("CANAL"));
    let (ef, eg) = (EntityRef::Face(im, f), EntityRef::Face(dm, g));
    assign_layer(scene, &ef, Some("CANAL".into()));
    assign_layer(scene, &eg, Some("CANAL".into()));
    let panel = &mut tray.layers;
    panel.refresh(&w);
    panel.current = Some("CANAL".into());
    panel.delete(&mut w);
    let scene = &w.viewport.scene;
    assert!(scene.doc.layer("CANAL").is_none());
    assert_eq!(layer_of(scene, &ef), DEFAULT_LAYER);
    assert_eq!(layer_of(scene, &eg), DEFAULT_LAYER);
}

#[test]
fn test_the_purge_button_sweeps_and_ctrl_z_brings_them_back() {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().confirms.push(true); // «Yes»
    w.dialogs = Box::new(dialogs.clone());
    let mut tray = Tray::default();
    for name in ["CVL_POINT", "HOLES", "Trees"] {
        w.viewport.scene.doc.layers.push(Layer::new(name));
    }
    let panel = &mut tray.layers;
    panel.refresh(&w);
    panel.purge(&mut w);
    assert_eq!(names(&w), [DEFAULT_LAYER]);
    assert_eq!(panel.rows.len(), 1);
    w.viewport.undo();
    panel.refresh(&w);
    assert_eq!(names(&w), [DEFAULT_LAYER, "CVL_POINT", "HOLES", "Trees"]);
}
