// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Edit ▸ Invert Selection — only what a click or a box could pick comes in
//! (nothing hidden, nothing on a hidden or locked layer, nothing outside the
//! group being edited).

use crate::common;

use std::collections::BTreeSet;

use common::{v3, window};
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::viewport::ViewportEvent;
use xtrude_app::window::KeySeq;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::layers::Layer;
use xtrude_model::scene::{Scene, SelectMode};

fn square(m: &mut Mesh, x0: f64) -> FaceId {
    m.add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 1., 0., 0.),
            v3(x0 + 1., 1., 0.),
            v3(x0, 1., 0.),
        ],
        &[],
    )
}

fn f(face: FaceId) -> EntityRef {
    EntityRef::Face(MeshId::LOOSE, face)
}

fn edges_of(s: &Scene, face: FaceId) -> Vec<EntityRef> {
    let m = s.mesh();
    m.edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).faces.contains(&face))
        .map(|e| EntityRef::Edge(MeshId::LOOSE, e))
        .collect()
}

fn loose(s: &Scene) -> BTreeSet<EntityRef> {
    let m = s.mesh();
    m.edge_ids()
        .into_iter()
        .map(|e| EntityRef::Edge(MeshId::LOOSE, e))
        .chain(m.face_ids().into_iter().map(f))
        .collect()
}

fn sel(s: &Scene) -> BTreeSet<EntityRef> {
    s.selection.iter().copied().collect()
}

fn group(s: &mut Scene, name: &str, faces_at: Option<f64>) -> Group {
    let mut m = Mesh::new();
    if let Some(x) = faces_at {
        square(&mut m, x);
    }
    let id = add_mesh(&mut s.doc.meshes, m);
    Group::new(id, Some(name))
}

#[test]
fn test_invert_swaps_selected_and_unselected() {
    let mut s = Scene::new();
    let a = square(s.mesh_mut(), 0.);
    let b = square(s.mesh_mut(), 5.);
    s.select([f(a)], SelectMode::Replace);
    let before = s.version;
    let n = s.invert_selection();
    assert!(!s.selection.contains(&f(a)));
    assert!(s.selection.contains(&f(b)));
    assert!(edges_of(&s, b).iter().all(|e| s.selection.contains(e))); // its edges too
    assert_eq!(n, s.selection.len());
    assert!(s.version > before); // the viewport repaints
}

#[test]
fn test_invert_twice_is_the_original_selection() {
    let mut s = Scene::new();
    let a = square(s.mesh_mut(), 0.);
    square(s.mesh_mut(), 5.);
    let mut first = vec![f(a)];
    first.extend(edges_of(&s, a));
    s.select(first, SelectMode::Replace);
    let original = sel(&s);
    s.invert_selection();
    s.invert_selection();
    assert_eq!(sel(&s), original);
}

#[test]
fn test_invert_with_nothing_selected_selects_everything() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0.);
    let g = group(&mut s, "box", Some(10.));
    let gid = g.id;
    s.doc.groups.push(g);
    s.invert_selection();
    let mut want = loose(&s);
    want.insert(EntityRef::Group(gid));
    assert_eq!(sel(&s), want);
}

#[test]
fn test_invert_with_everything_selected_clears() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0.);
    let all: Vec<EntityRef> = loose(&s).into_iter().collect();
    s.select(all, SelectMode::Replace);
    assert_eq!(s.invert_selection(), 0);
    assert!(s.selection.is_empty());
}

// ---- what cannot be picked stays out ---------------------------------------

#[test]
fn test_hidden_group_stays_out() {
    let mut s = Scene::new();
    let shown = group(&mut s, "visible", None);
    let mut hidden = group(&mut s, "hidden", None);
    hidden.hidden = true;
    let (a, b) = (shown.id, hidden.id);
    s.doc.groups.extend([shown, hidden]);
    s.invert_selection();
    assert!(s.selection.contains(&EntityRef::Group(a)));
    assert!(!s.selection.contains(&EntityRef::Group(b)));
}

#[test]
fn test_hidden_face_stays_out_unless_the_hidden_view_is_on() {
    let mut s = Scene::new();
    let face = square(s.mesh_mut(), 0.);
    s.mesh_mut().face_mut(face).attrs.hidden = true;
    s.invert_selection();
    assert!(!s.selection.contains(&f(face)));
    s.clear_selection();
    s.show_hidden_geometry = true;
    s.invert_selection();
    assert!(s.selection.contains(&f(face)));
}

#[test]
fn test_hidden_edge_stays_out_unless_the_hidden_view_is_on() {
    let mut s = Scene::new();
    let face = square(s.mesh_mut(), 0.);
    let edge = edges_of(&s, face)[0];
    if let EntityRef::Edge(_, e) = edge {
        s.mesh_mut().edge_mut(e).hidden = true;
    }
    s.invert_selection();
    assert!(!s.selection.contains(&edge));
    assert!(s.selection.contains(&f(face)));
    s.clear_selection();
    s.show_hidden_geometry = true;
    s.invert_selection();
    assert!(s.selection.contains(&edge));
}

#[test]
fn test_locked_and_hidden_layers_stay_out() {
    let mut s = Scene::new();
    let mut backdrop = Layer::new("Backdrop");
    backdrop.locked = true;
    let mut mob = Layer::new("Fixtures");
    mob.visible = false;
    s.doc.layers.extend([backdrop, mob]);
    let free = square(s.mesh_mut(), 0.);
    let locked = square(s.mesh_mut(), 5.);
    let off = square(s.mesh_mut(), 10.);
    s.mesh_mut().face_mut(locked).attrs.layer = Some("Backdrop".into());
    s.mesh_mut().face_mut(off).attrs.layer = Some("Fixtures".into());
    s.invert_selection();
    assert!(s.selection.contains(&f(free)));
    assert!(!s.selection.contains(&f(locked)));
    assert!(!s.selection.contains(&f(off)));
}

#[test]
fn test_inside_a_group_only_its_contents_count() {
    let mut s = Scene::new();
    square(s.mesh_mut(), 0.); // loose, outside the group
    let mut outer = group(&mut s, "exterior", Some(10.));
    let child = group(&mut s, "child", None);
    let (oid, cid, om) = (outer.id, child.id, outer.mesh);
    outer.children.push(child);
    let other = group(&mut s, "other", None);
    let other_id = other.id;
    s.doc.groups.extend([outer, other]);
    s.begin_group_edit(oid);
    s.invert_selection();
    let gs: Vec<GroupId> = s
        .selection
        .iter()
        .filter_map(|e| match e {
            EntityRef::Group(g) => Some(*g),
            _ => None,
        })
        .collect();
    assert!(gs.contains(&cid)); // the open group's child
    let m = s.doc.mesh(om);
    assert!(m
        .face_ids()
        .iter()
        .all(|fc| s.selection.contains(&EntityRef::Face(om, *fc)))); // its own geometry
    assert!(!gs.contains(&other_id)); // the model around it: no
    assert!(!gs.contains(&oid));
    s.end_group_edit();
}

// ---- the window: menu, shortcut, status bar --------------------------------

#[test]
fn test_edit_menu_offers_it_with_the_usual_shortcut() {
    let mut w = window();
    let owners = w.actions.owners(KeySeq::parse("Ctrl+Shift+I").unwrap());
    assert_eq!(owners.len(), 1); // one owner: never ambiguous
    let a = square(w.viewport.scene.mesh_mut(), 0.);
    let b = square(w.viewport.scene.mesh_mut(), 5.);
    w.viewport.scene.select([f(a)], SelectMode::Replace);
    let ev = KeyEvent::new(
        Key::Char('I'),
        "",
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        },
    );
    let seen_before = w.viewport.scene.version;
    // the window routes the chord to the action, and the tray hears it
    let owners: Vec<String> = w
        .actions
        .for_key(&ev)
        .iter()
        .map(|x| x.id.clone())
        .collect();
    assert_eq!(owners, vec!["invert_selection"]);
    w.seen_events.clear();
    w.trigger(&owners[0]);
    let sel = &w.viewport.scene.selection;
    assert!(sel.contains(&f(b)) && !sel.contains(&f(a)));
    assert!(w.viewport.scene.version > seen_before);
    assert!(w
        .seen_events
        .iter()
        .any(|e| matches!(e, ViewportEvent::SceneVersionChanged(_))));
    assert!(w.status_message.is_some());
}
