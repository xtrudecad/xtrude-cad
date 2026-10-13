// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Visibility, locking, assignment, the annotations' layers and the picks that
//! honour them. The document round trip lives in crates/xtrude-io.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use xtrude_app::fontmetrics::FontMetrics;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{assign_layer, layer_of};
use xtrude_model::dimension::Dimension;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::layers::{Layer, DEFAULT_LAYER};
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::{Scene, SelectMode};
use xtrude_model::textlabel::TextLabel;

fn slab(s: &mut Scene, x0: f64) -> FaceId {
    s.mesh_mut().add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 2., 0., 0.),
            v3(x0 + 2., 2., 0.),
            v3(x0, 2., 0.),
        ],
        &[],
    )
}

fn f(x: FaceId) -> EntityRef {
    EntityRef::Face(MeshId::LOOSE, x)
}

fn tag(s: &mut Scene, e: EntityRef, name: &str) {
    assign_layer(s, &e, Some(name.into()));
}

fn layer(name: &str, visible: bool, locked: bool) -> Layer {
    let mut l = Layer::new(name);
    l.visible = visible;
    l.locked = locked;
    l
}

#[test]
fn test_entities_default_to_layer_zero() {
    let mut s = Scene::new();
    let fc = slab(&mut s, 0.);
    let e = EntityRef::Edge(MeshId::LOOSE, s.mesh().edge_ids()[0]);
    assert_eq!(layer_of(&s, &f(fc)), DEFAULT_LAYER);
    assert_eq!(layer_of(&s, &e), DEFAULT_LAYER);
    assert!(s.entity_visible(&f(fc)) && s.entity_selectable(&e));
}

#[test]
fn test_hidden_layer_filters_render_views() {
    let mut s = Scene::new();
    let f1 = slab(&mut s, 0.);
    let f2 = slab(&mut s, 5.);
    s.doc.layers.push(layer("Structure", false, false));
    tag(&mut s, f(f2), "Structure");
    let m = s.mesh();
    let own: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).faces.contains(&f2))
        .collect();
    for e in own {
        tag(&mut s, EntityRef::Edge(MeshId::LOOSE, e), "Structure");
    }
    let faces = s.render_faces();
    assert!(faces.contains(&(MeshId::LOOSE, f1)) && !faces.contains(&(MeshId::LOOSE, f2)));
    assert_eq!(s.render_edges().len(), 4); // only slab 1's edges
    assert!(!s.entity_selectable(&f(f2)));
}

#[test]
fn test_locked_layer_visible_but_unselectable() {
    let mut s = Scene::new();
    let fc = slab(&mut s, 0.);
    s.doc.layers.push(layer("Backdrop", true, true));
    tag(&mut s, f(fc), "Backdrop");
    assert!(s.entity_visible(&f(fc)));
    assert!(!s.entity_selectable(&f(fc)));
}

#[test]
fn test_group_layer_hides_whole_group() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let id = add_mesh(&mut s.doc.meshes, m);
    let g = Group::new(id, None);
    let gid = g.id;
    s.doc.groups.push(g);
    s.doc.layers.push(layer("Fixtures", false, false));
    tag(&mut s, EntityRef::Group(gid), "Fixtures");
    assert!(s.render_faces().is_empty());
    assert!(!s.entity_selectable(&EntityRef::Group(gid)));
}

#[test]
fn test_edge_layer_survives_split_and_snapshot() {
    let mut s = Scene::new();
    let e = s
        .mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(4., 0., 0.))
        .unwrap();
    tag(&mut s, EntityRef::Edge(MeshId::LOOSE, e), "Wiring");
    let snap = s.mesh().clone();
    let m = s.mesh_mut();
    let mid = m.vertex(v3(2., 0., 0.));
    m.split_edge_at(e, mid);
    let all = |s: &Scene| {
        let m = s.mesh();
        m.edge_ids()
            .into_iter()
            .all(|x| m.edge(x).layer.as_deref() == Some("Wiring"))
    };
    assert!(all(&s));
    *s.mesh_mut() = snap;
    assert!(all(&s));
}

#[test]
fn test_annotations_take_layers_like_any_entity() {
    let mut s = Scene::new();
    let dim = Dimension::new(v3(0., 0., 0.), v3(2., 0., 0.), v3(0., -1., 0.));
    let lab = TextLabel::new(v3(1., 0., 0.), v3(0.5, 0.5, 1.), "Socket");
    let (d, l) = (EntityRef::Dimension(dim.id), EntityRef::TextLabel(lab.id));
    s.doc.dimensions.push(dim);
    s.doc.text_labels.push(lab);
    assert!(layer_of(&s, &d) == DEFAULT_LAYER && layer_of(&s, &l) == DEFAULT_LAYER);
    s.doc.layers.push(Layer::new("Annotations"));
    tag(&mut s, d, "Annotations");
    tag(&mut s, l, "Annotations");
    assert_eq!(s.doc.dimensions[0].layer.as_deref(), Some("Annotations"));
    assert_eq!(s.doc.text_labels[0].layer.as_deref(), Some("Annotations"));
    assert!(s.entity_visible(&d) && s.entity_selectable(&l));
    s.doc.layer_mut("Annotations").unwrap().visible = false;
    assert!(!s.entity_visible(&d) && !s.entity_visible(&l));
    assert!(!s.entity_selectable(&d));
    s.doc.layer_mut("Annotations").unwrap().visible = true;
    s.doc.layer_mut("Annotations").unwrap().locked = true;
    assert!(s.entity_visible(&l) && !s.entity_selectable(&l));
    s.doc.layer_mut("Annotations").unwrap().locked = false;
    // A scene that hides the layer hides the annotations on recall.
    let mut cam = OrbitCamera::default();
    s.doc.layer_mut("Annotations").unwrap().visible = false;
    let clean = SavedView::capture("Clean top", &s, &cam);
    s.doc.layer_mut("Annotations").unwrap().visible = true;
    clean.apply(&mut s, &mut cam);
    assert!(!s.entity_visible(&d));
    s.doc.layer_mut("Annotations").unwrap().visible = true;
    // Back to the default layer = no tag stored.
    tag(&mut s, d, DEFAULT_LAYER);
    assert!(s.doc.dimensions[0].layer.is_none());
}

#[test]
fn test_hidden_layer_hides_annotations_from_picks() {
    let mut vp = stub();
    let lab = TextLabel::new(v3(3., 0., 0.), v3(-2., 0., 0.), "Socket");
    let dim = Dimension::new(v3(0., 0., 0.), v3(2., 0., 0.), v3(0., 0., 1.));
    let (lid, did) = (lab.id, dim.id);
    vp.scene.doc.text_labels.push(lab);
    vp.scene.doc.dimensions.push(dim);
    vp.scene.doc.layers.push(Layer::new("Annotations"));
    tag(&mut vp.scene, EntityRef::TextLabel(lid), "Annotations");
    tag(&mut vp.scene, EntityRef::Dimension(did), "Annotations");
    // the table: x → pixel; the dimension's offset points (z=1) on y=150
    FakeProbe::of(&mut vp).world_to_pixel(|p| {
        let (x, y) = match (p.x * 1e6).round() as i64 {
            3_000_000 => (300.0, 100.0),
            1_000_000 => (100.0, 100.0),
            0 => (0.0, 200.0),
            2_000_000 => (200.0, 200.0),
            _ => return None,
        };
        Some(if p.z > 0.5 { (x, 150.0) } else { (x, y) })
    });
    let w = FontMetrics::new(9.0, true).horizontal_advance("Socket");
    let at = (100. - 6. - w / 2., 96.);
    assert_eq!(vp.pick_text_label(at.0, at.1, true), Some(lid));
    assert_eq!(vp.pick_dimension(100., 150.), Some(did)); // on the dimension line
    vp.scene.doc.layer_mut("Annotations").unwrap().visible = false;
    assert_eq!(vp.pick_text_label(at.0, at.1, true), None);
    assert_eq!(vp.pick_dimension(100., 150.), None);
    vp.scene.doc.layer_mut("Annotations").unwrap().visible = true;
    vp.scene.doc.layer_mut("Annotations").unwrap().locked = true;
    assert_eq!(vp.pick_text_label(at.0, at.1, true), None);
    assert_eq!(vp.pick_dimension(100., 150.), None);
}

#[test]
fn test_layers_tray_assigns_and_releases_annotations() {
    // Layers ▸ Assign moves the selected dimensions and texts to the
    // current layer, and deleting that layer sends them back to the
    // default one.
    let mut w = common::window();
    let mut tray = xtrude_app::tray::Tray::default();
    let dim = Dimension::new(v3(0., 0., 0.), v3(2., 0., 0.), v3(0., -1., 0.));
    let lab = TextLabel::new(v3(1., 0., 0.), v3(0.5, 0.5, 1.), "Socket");
    let (d, l) = (EntityRef::Dimension(dim.id), EntityRef::TextLabel(lab.id));
    let scene = &mut w.viewport.scene;
    scene.doc.dimensions.push(dim);
    scene.doc.text_labels.push(lab);
    scene.doc.layers.push(Layer::new("Annotations"));
    let panel = &mut tray.layers;
    panel.refresh(&w);
    assert!(panel.rows.iter().any(|r| r.name == "Annotations"));
    panel.current = Some("Annotations".into());
    w.viewport.scene.select([d, l], SelectMode::Replace);
    panel.assign(&mut w);
    let scene = &w.viewport.scene;
    assert!(layer_of(scene, &d) == "Annotations" && layer_of(scene, &l) == "Annotations");
    panel.delete(&mut w);
    let scene = &w.viewport.scene;
    assert!(scene.doc.layer("Annotations").is_none());
    assert!(layer_of(scene, &d) == DEFAULT_LAYER && layer_of(scene, &l) == DEFAULT_LAYER);
}
