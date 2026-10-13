// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A selection after Ctrl+S does not make the saved drawing "modified" — it
//! bumps the render version, not the content version — while a real edit does.

use crate::common;

use common::{v3, window, TmpDir};
use std::path::PathBuf;
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::SelectMode;

/// Six groups saved to plan.xtr.
fn doc(dir: &TmpDir) -> PathBuf {
    let mut w0 = window();
    let sc = &mut w0.viewport.scene;
    for i in 0..6 {
        let x = i as f64 * 3.0;
        let mut m = Mesh::new();
        m.add_face(
            &[
                v3(x, 0., 0.),
                v3(x + 2., 0., 0.),
                v3(x + 2., 2., 0.),
                v3(x, 2., 0.),
            ],
            &[],
        );
        let g = Group::new(add_mesh(&mut sc.doc.meshes, m), Some(&format!("B{i}")));
        sc.doc.groups.push(g);
    }
    sc.version += 1;
    let path = dir.path().join("plan.xtr");
    w0.do_save(&path);
    path
}

fn open(path: &std::path::Path) -> MainWindow {
    let mut w = window();
    assert!(w.open_path(path));
    w
}

fn first_group(w: &MainWindow) -> EntityRef {
    EntityRef::Group(w.viewport.scene.doc.groups[0].id)
}

#[test]
fn test_selection_after_ctrl_s_does_not_ask_to_save_again() {
    let dir = TmpDir::new();
    let mut w = open(&doc(&dir));
    let g = first_group(&w);
    w.viewport.scene.select([g], SelectMode::Replace);
    w.trigger("delete_selection"); // a real edit
    assert!(w.is_dirty());
    w.trigger("save"); // Ctrl+S
    assert!(!w.is_dirty());
    // What a user does before closing: click empty space, click an object,
    // Ctrl+Shift+I, Esc.
    let sc = &mut w.viewport.scene;
    sc.clear_selection();
    let g = EntityRef::Group(sc.doc.groups[0].id);
    sc.select([g], SelectMode::Replace);
    sc.select([], SelectMode::Replace);
    sc.invert_selection();
    sc.clear_selection();
    w.update_title();
    assert!(
        !w.is_dirty(),
        "a selection made the saved drawing 'modified'"
    );
    assert!(!w.title.contains('*'));
    // The inverse: a real edit after the save does make it dirty.
    let g = first_group(&w);
    w.viewport.scene.select([g], SelectMode::Replace);
    w.trigger("delete_selection");
    assert!(w.is_dirty());
}

#[test]
fn test_selection_keeps_bumping_the_render_version() {
    let dir = TmpDir::new();
    let mut w = open(&doc(&dir));
    let (before, content) = (w.viewport.scene.version, w.viewport.scene.content_version());
    let g = first_group(&w);
    w.viewport.scene.select([g], SelectMode::Replace);
    assert_eq!(w.viewport.scene.version, before + 1);
    assert_eq!(w.viewport.scene.content_version(), content);
}

#[test]
fn test_marking_saved_with_scene_version_still_means_clean() {
    let dir = TmpDir::new();
    let mut w = open(&doc(&dir));
    let g = first_group(&w);
    w.viewport.scene.select([g], SelectMode::Replace);
    w.viewport.scene.clear_selection();
    w.set_saved_version(w.viewport.scene.version as i64);
    assert!(!w.is_dirty());
    w.set_saved_version(-1); // "never saved" stays dirty
    assert!(w.is_dirty());
}
