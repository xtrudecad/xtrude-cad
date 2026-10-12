// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Changing an object's layer goes through one undoable command, offered on
//! the right-click as a Layer submenu with «New layer…», the Properties field
//! and the Layers panel's Assign button.

use crate::common;

use common::{v3, window};
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::tray::layers::Layers;
use xtrude_app::tray::properties::Properties;
use xtrude_app::viewport::Viewport;
use xtrude_app::window::{CtxItem, MainWindow, ScriptedDialogs};
use xtrude_core::Mesh;
use xtrude_model::commands::{layer_of, AssignLayerCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::layers::Layer;
use xtrude_model::scene::SelectMode;

fn empty_group(vp: &mut Viewport, layer: Option<&str>) -> GroupId {
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, Mesh::new()), Some("box"));
    g.layer = layer.map(String::from);
    let id = g.id;
    vp.scene.doc.groups.push(g);
    id
}

#[test]
fn test_assign_layer_command_moves_a_mixed_selection_and_undoes_exactly() {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.scene.doc.layers.push(Layer::new("Shells"));
    let face = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let edge = vp.scene.mesh().edge_ids()[0];
    let g = empty_group(vp, Some("Shells"));
    let ents = vec![
        EntityRef::Face(MeshId::LOOSE, face),
        EntityRef::Edge(MeshId::LOOSE, edge),
        EntityRef::Group(g),
    ];
    vp.execute(&mut AssignLayerCommand {
        entities: ents.clone(),
        layer: "Fixtures".into(),
    });
    for e in &ents {
        assert_eq!(layer_of(&vp.scene, e), "Fixtures");
    }
    assert!(vp.scene.layer("Fixtures").is_some()); // created on the way
    vp.undo();
    assert_eq!(layer_of(&vp.scene, &ents[0]), "Layer 0");
    assert_eq!(layer_of(&vp.scene, &ents[1]), "Layer 0");
    assert_eq!(layer_of(&vp.scene, &ents[2]), "Shells");
    assert!(vp.scene.layer("Fixtures").is_none()); // …and taken back
    vp.redo();
    assert_eq!(layer_of(&vp.scene, &ents[2]), "Fixtures");
}

fn layer_submenu(w: &MainWindow) -> Vec<CtxItem> {
    w.context_menu()
        .into_iter()
        .find_map(|i| match i {
            CtxItem::Menu(t, items) if t == "Layer" => Some(items),
            _ => None,
        })
        .expect("a Layer submenu")
}

#[test]
fn test_right_click_layer_submenu_and_new_layer() {
    let mut w = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    w.viewport.scene.doc.layers.push(Layer::new("Shells"));
    let g = empty_group(&mut w.viewport, None);
    w.viewport
        .scene
        .select([EntityRef::Group(g)], SelectMode::Replace);
    let sub = layer_submenu(&w);
    let names: Vec<String> = sub
        .iter()
        .filter(|i| !matches!(i, CtxItem::Separator))
        .map(|i| i.text().to_string())
        .collect();
    assert_eq!(&names[..2], ["Layer 0", "Shells"]);
    assert_eq!(names.last().unwrap(), "New layer…");
    let ticked: Vec<String> = sub
        .iter()
        .filter_map(|i| match i {
            CtxItem::Entry {
                text,
                checked: Some(true),
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ticked, vec!["Layer 0".to_string()]);
    w.trigger("layer:assign:Shells");
    assert_eq!(layer_of(&w.viewport.scene, &EntityRef::Group(g)), "Shells");

    dialogs.borrow_mut().texts.push(Some("Inserts".into()));
    w.trigger("layer:new");
    assert_eq!(layer_of(&w.viewport.scene, &EntityRef::Group(g)), "Inserts");
    assert!(w.viewport.scene.layer("Inserts").is_some());
    w.viewport.undo();
    assert_eq!(layer_of(&w.viewport.scene, &EntityRef::Group(g)), "Shells");
    assert!(w.viewport.scene.layer("Inserts").is_none());
}

#[test]
fn test_properties_shows_the_layer_and_changes_it() {
    let mut w = window();
    w.viewport.scene.doc.layers.push(Layer::new("Shells"));
    let face = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let g = empty_group(&mut w.viewport, None);
    let fref = EntityRef::Face(MeshId::LOOSE, face);
    let mut panel = Properties::default();
    w.viewport.scene.select([fref], SelectMode::Replace);
    panel.refresh(&w);
    assert!(panel.layer.visible);
    assert_eq!(
        panel.layer.items[panel.layer.current].1.as_deref(),
        Some("Layer 0")
    );
    let idx = panel
        .layer
        .items
        .iter()
        .position(|(_, d)| d.as_deref() == Some("Shells"))
        .unwrap();
    panel.layer_picked(&mut w, idx); // the user picks Shells
    assert_eq!(layer_of(&w.viewport.scene, &fref), "Shells");
    w.viewport.undo();
    assert_eq!(layer_of(&w.viewport.scene, &fref), "Layer 0");
    // a mixed selection reads as such, and picking moves everything
    w.viewport.scene.doc.find_group_mut(g).unwrap().layer = Some("Shells".into());
    w.viewport
        .scene
        .select([fref, EntityRef::Group(g)], SelectMode::Replace);
    panel.refresh(&w);
    assert_eq!(panel.layer.items[panel.layer.current].1, None);
    assert_eq!(panel.layer.items[panel.layer.current].0, "(several)");
    let idx = panel
        .layer
        .items
        .iter()
        .position(|(_, d)| d.as_deref() == Some("Shells"))
        .unwrap();
    panel.layer_picked(&mut w, idx);
    assert_eq!(layer_of(&w.viewport.scene, &fref), "Shells");
    assert_eq!(layer_of(&w.viewport.scene, &EntityRef::Group(g)), "Shells");
    w.viewport.scene.clear_selection();
    panel.refresh(&w);
    assert!(!panel.layer.visible);
}

#[test]
fn test_layers_panel_assign_button_speaks_and_undoes() {
    let mut w = window();
    w.viewport.scene.doc.layers.push(Layer::new("Shells"));
    let g = empty_group(&mut w.viewport, None);
    let gref = EntityRef::Group(g);
    let mut panel = Layers::default();
    panel.refresh(&w);
    // nothing highlighted in the list: it says so instead of doing nothing
    panel.current = None;
    w.viewport.scene.select([gref], SelectMode::Replace);
    w.status_message = None;
    panel.assign(&mut w);
    assert_eq!(layer_of(&w.viewport.scene, &gref), "Layer 0");
    assert!(w
        .status_message
        .take()
        .unwrap()
        .text
        .to_lowercase()
        .contains("layer"));
    // highlight Shells, nothing selected in the model: says so too
    panel.current = Some("Shells".into());
    w.viewport.scene.clear_selection();
    panel.assign(&mut w);
    assert!(w
        .status_message
        .take()
        .unwrap()
        .text
        .to_lowercase()
        .contains("select"));
    // both in place: moves, undoably
    w.viewport.scene.select([gref], SelectMode::Replace);
    panel.assign(&mut w);
    assert_eq!(layer_of(&w.viewport.scene, &gref), "Shells");
    w.viewport.undo();
    assert_eq!(layer_of(&w.viewport.scene, &gref), "Layer 0");
}
