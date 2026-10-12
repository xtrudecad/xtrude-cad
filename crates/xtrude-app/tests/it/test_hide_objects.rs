// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hide on objects, faces and edges, the hidden views, and saved views that
//! remember what is hidden. The .igz tests live in crates/xtrude-io. The chunk
//! and ghost-pass tests read the frame (`Viewport::frame_data`) the window's
//! viewport draws.

use crate::common;

use common::{v3, window};
use glam::DMat4;
use serde_json::json;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::Mesh;
use xtrude_model::commands::HideCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, Group};
use xtrude_model::history::History;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::{Scene, SelectMode};
use xtrude_render::frame::{Source, Stipple};

fn boxm(m: &mut Mesh, x0: f64) {
    let p: Vec<_> = (0..8)
        .map(|i| v3(x0 + (i & 1) as f64, ((i >> 1) & 1) as f64, (i >> 2) as f64))
        .collect();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        m.add_face(&q.map(|i| p[i]), &[]);
    }
}

fn group(s: &mut Scene, name: &str, x0: f64) -> Group {
    let mut m = Mesh::new();
    boxm(&mut m, x0);
    let id = add_mesh(&mut s.doc.meshes, m);
    Group::new(id, Some(name))
}

fn push(s: &mut Scene, g: Group) -> GroupId {
    let id = g.id;
    s.doc.groups.push(g);
    s.version += 1;
    id
}

fn hidden(s: &Scene, g: GroupId) -> bool {
    s.doc.find_group(g).unwrap().hidden
}

#[test]
fn test_hide_command_takes_the_object_out_of_sight_and_undo_brings_it_back() {
    let mut s = Scene::new();
    let mut h = History::new();
    let g = group(&mut s, "box", 0.);
    let g = push(&mut s, g);
    let e = EntityRef::Group(g);
    s.select([e], SelectMode::Replace);
    assert!(s.entity_visible(&e) && s.entity_selectable(&e));
    h.execute(
        &mut s,
        &mut HideCommand {
            entities: vec![e],
            hidden: true,
        },
    );
    assert!(hidden(&s, g));
    assert!(!s.entity_visible(&e) && !s.entity_selectable(&e));
    assert!(!s.selection.contains(&e)); // nothing invisible stays selected
    assert!(s.bounds().is_none()); // gone from the extents too
    assert!(h.undo(&mut s));
    assert!(!hidden(&s, g) && s.entity_visible(&e));
    assert!(h.redo(&mut s));
    assert!(hidden(&s, g));
}

#[test]
fn test_hide_command_also_hides_faces_and_edges() {
    let mut s = Scene::new();
    let mut h = History::new();
    boxm(s.mesh_mut(), 0.);
    let edge = s.mesh().edge_ids()[0];
    let face = s.mesh().face_ids()[0];
    let (ee, fe) = (
        EntityRef::Edge(MeshId::LOOSE, edge),
        EntityRef::Face(MeshId::LOOSE, face),
    );
    h.execute(
        &mut s,
        &mut HideCommand {
            entities: vec![ee, fe],
            hidden: true,
        },
    );
    assert!(s.mesh().edge(edge).hidden);
    assert!(s.mesh().face(face).attrs.hidden);
    assert!(!s.entity_visible(&fe) && !s.entity_selectable(&fe));
    assert!(s.entity_hidden(&fe));
    assert!(h.undo(&mut s));
    assert!(!s.mesh().face(face).attrs.hidden && !s.mesh().edge(edge).hidden);
}

#[test]
fn test_the_hidden_view_makes_hidden_things_selectable_but_never_visible() {
    let mut s = Scene::new();
    let g = group(&mut s, "box", 0.);
    let g = push(&mut s, g);
    boxm(s.mesh_mut(), 5.);
    let face = s.mesh().face_ids()[0];
    s.doc.find_group_mut(g).unwrap().hidden = true;
    s.mesh_mut().face_mut(face).attrs.hidden = true;
    let (ge, fe) = (EntityRef::Group(g), EntityRef::Face(MeshId::LOOSE, face));
    assert!(!s.entity_selectable(&ge) && !s.entity_selectable(&fe));
    s.show_hidden_objects = true;
    assert!(s.entity_selectable(&ge) && !s.entity_visible(&ge));
    assert!(!s.entity_selectable(&fe));
    s.show_hidden_geometry = true;
    assert!(s.entity_selectable(&fe) && !s.entity_visible(&fe));
}

#[test]
fn test_every_group_gets_its_own_uid_and_a_copy_a_new_one() {
    let mut s = Scene::new();
    let mut g = group(&mut s, "box", 0.);
    let c = copy_group(&mut s.doc.meshes, &mut g, None, false);
    assert!(!g.uid.is_empty() && !c.uid.is_empty() && g.uid != c.uid);
    assert_eq!(g.uid.len(), 16);
    g.hidden = true;
    assert!(copy_group(&mut s.doc.meshes, &mut g, None, false).hidden);
}

#[test]
fn test_a_scene_remembers_which_objects_are_hidden() {
    let mut s = Scene::new();
    let (a, b) = (group(&mut s, "a", 0.), group(&mut s, "b", 3.));
    let (a, b) = (push(&mut s, a), push(&mut s, b));
    let mut cam = OrbitCamera::default();
    s.doc.find_group_mut(b).unwrap().hidden = true;
    let view = SavedView::capture("without box b", &s, &cam);
    let buid = s.doc.find_group(b).unwrap().uid.clone();
    assert_eq!(view.hidden_objects, Some(vec![buid.clone()]));
    s.doc.find_group_mut(b).unwrap().hidden = false;
    s.doc.find_group_mut(a).unwrap().hidden = true;
    view.apply(&mut s, &mut cam);
    assert!(hidden(&s, b) && !hidden(&s, a)); // exactly what was saved
    let raw = view.to_json();
    assert_eq!(raw["hidden_objects"], json!([buid.clone()]));
    assert_eq!(SavedView::from_json(&raw).hidden_objects, Some(vec![buid]));
}

#[test]
fn test_a_scene_from_before_this_field_leaves_hidden_objects_alone() {
    let mut s = Scene::new();
    let g = group(&mut s, "box", 0.);
    let g = push(&mut s, g);
    let mut cam = OrbitCamera::default();
    let old = SavedView::from_json(&json!({"name": "old", "target": [0, 0, 0],
                                            "distance": 10.0, "yaw": 0.0, "pitch": 0.3}));
    assert!(old.hidden_objects.is_none());
    s.doc.find_group_mut(g).unwrap().hidden = true;
    old.apply(&mut s, &mut cam);
    assert!(hidden(&s, g)); // hands-off
}

#[test]
fn test_hiding_a_container_hides_its_nested_placements_in_the_viewport() {
    let mut w = window();
    let s = &mut w.viewport.scene;
    let mut letter = group(s, "leaf", 0.);
    letter.xform = Some(DMat4::IDENTITY);
    let lid = letter.id;
    let cm = add_mesh(&mut s.doc.meshes, Mesh::new());
    let mut container = Group::new(cm, Some("tree"));
    container.adopt(vec![letter]);
    let c = push(s, container);
    let proxies = |w: &xtrude_app::window::MainWindow| {
        w.viewport
            .placements()
            .into_iter()
            .filter(|p| p.owner == c && p.nested)
            .collect::<Vec<_>>()
    };
    assert!(!proxies(&w).is_empty() && !proxies(&w).iter().any(|p| p.hidden));
    w.viewport.run(HideCommand {
        entities: vec![EntityRef::Group(c)],
        hidden: true,
    });
    assert!(proxies(&w).iter().all(|p| p.hidden));
    // a child hidden on its own stays hidden inside a visible parent
    assert!(w.viewport.undo());
    w.viewport.run(HideCommand {
        entities: vec![EntityRef::Group(lid)],
        hidden: true,
    });
    assert!(proxies(&w).iter().all(|p| p.hidden));
    assert!(w.viewport.scene.entity_visible(&EntityRef::Group(c)));
}

#[test]
fn test_edit_menu_hide_unhide_last_and_all() {
    let mut w = window();
    let s = &mut w.viewport.scene;
    let (a, b) = (group(s, "a", 0.), group(s, "b", 3.));
    let (a, b) = (push(s, a), push(s, b));
    boxm(s.mesh_mut(), 6.);
    let edge = s.mesh().edge_ids()[0];
    let ee = EntityRef::Edge(MeshId::LOOSE, edge);
    s.version += 1;
    w.viewport
        .scene
        .select([EntityRef::Group(a)], SelectMode::Replace);
    w.trigger("hide");
    assert!(hidden(&w.viewport.scene, a) && !hidden(&w.viewport.scene, b));
    w.viewport
        .scene
        .select([EntityRef::Group(b), ee], SelectMode::Replace);
    w.trigger("hide");
    assert!(hidden(&w.viewport.scene, b) && w.viewport.scene.mesh().edge(edge).hidden);
    w.trigger("unhide_last"); // the second Hide comes back
    let s = &w.viewport.scene;
    assert!(!hidden(s, b) && !s.mesh().edge(edge).hidden && hidden(s, a));
    w.trigger("unhide_last");
    assert!(!hidden(&w.viewport.scene, a));
    w.viewport.scene.select(
        [EntityRef::Group(a), EntityRef::Group(b)],
        SelectMode::Replace,
    );
    w.trigger("hide");
    w.trigger("unhide_all");
    assert!(!hidden(&w.viewport.scene, a) && !hidden(&w.viewport.scene, b));
    assert!(w.viewport.undo()); // Unhide All is one step
    assert!(hidden(&w.viewport.scene, a) && hidden(&w.viewport.scene, b));
}

/// How many face and line vertices the next frame draws for the scene,
/// and how many of them go through the see-through ghost passes.
fn drawn(w: &mut xtrude_app::window::MainWindow) -> ((u32, u32), (u32, u32)) {
    let f = w.viewport.frame_data([200, 200], false);
    let count = |src: Source, st: Stipple| {
        f.draws
            .iter()
            .filter(|d| d.source == src && d.stipple == st)
            .map(|d| d.count)
            .sum::<u32>()
    };
    (
        (
            count(Source::SceneFaces, Stipple::None),
            count(Source::SceneLines, Stipple::None),
        ),
        (
            count(Source::SceneFaces, Stipple::Grid),
            count(Source::SceneLines, Stipple::Dots),
        ),
    )
}

#[test]
fn test_hidden_faces_leave_a_groups_chunk_but_keep_their_edges() {
    let mut w = window();
    let s = &mut w.viewport.scene;
    let g = group(s, "box", 0.);
    let mid = g.mesh;
    push(s, g);
    let ((faces, edges), _) = drawn(&mut w);
    assert_eq!(faces, 6 * 2 * 3);
    let f0 = w.viewport.scene.doc.mesh(mid).face_ids()[0];
    w.viewport.run(HideCommand {
        entities: vec![EntityRef::Face(mid, f0)],
        hidden: true,
    });
    let ((after, after_edges), _) = drawn(&mut w);
    assert_eq!(after, 5 * 2 * 3); // rebuilt without it
    assert_eq!(after_edges, edges); // its edges stay drawn
    assert!(w.viewport.undo());
    assert_eq!(drawn(&mut w).0 .0, 6 * 2 * 3);
}

#[test]
fn test_ghost_pass_draws_and_unhide_selected_brings_them_back() {
    let mut w = window();
    let s = &mut w.viewport.scene;
    let g = group(s, "box", 0.);
    let g = push(s, g);
    boxm(s.mesh_mut(), 5.);
    let face = s.mesh().face_ids()[0];
    s.version += 1;
    let both = [EntityRef::Group(g), EntityRef::Face(MeshId::LOOSE, face)];
    w.viewport.scene.select(both, SelectMode::Replace);
    w.trigger("hide");
    assert!(hidden(&w.viewport.scene, g) && w.viewport.scene.mesh().face(face).attrs.hidden);
    assert_eq!(drawn(&mut w).1, (0, 0)); // view off: nothing
    w.trigger("show_hidden_groups");
    w.trigger("show_hidden_edges_faces");
    let s = &w.viewport.scene;
    assert!(s.show_hidden_objects && s.show_hidden_geometry);
    let (ghost_faces, ghost_edges) = drawn(&mut w).1;
    assert_eq!(ghost_faces, 6 * 2 * 3 + 2 * 3); // the box's faces + one face
    assert_eq!(ghost_edges, 12 * 2); // the object's edges, dotted
                                     // selectable again → Unhide ▸ Selected
    w.viewport.scene.select(both, SelectMode::Replace);
    w.trigger("unhide_selected");
    assert!(!hidden(&w.viewport.scene, g) && !w.viewport.scene.mesh().face(face).attrs.hidden);
    assert_eq!(drawn(&mut w).1, (0, 0));
}
