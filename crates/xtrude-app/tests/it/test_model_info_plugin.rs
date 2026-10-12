// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Model Info numbers must agree with the rest of the app.
//!
//! Each test in the second half pins one of the wrong numbers the original
//! contribution reported: materials read from a key Xtrude CAD never writes
//! (always empty), totals double-counted while a group was open for
//! editing, and colour swatches assumed 0–255 ints where Xtrude CAD paints
//! with floats 0–1.

use crate::common;

use common::v3;
use xtrude_app::window::actions::MenuItem;
use xtrude_app::window::model_info::{collect_stats, stats_to_text};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

fn scene() -> Scene {
    xtrude_model::settings::use_memory_store();
    Scene::new()
}

fn quad(mesh: &mut Mesh, z: f64, o: f64, size: f64) -> FaceId {
    mesh.add_face(
        &[
            v3(o, o, z),
            v3(o + size, o, z),
            v3(o + size, o + size, z),
            v3(o, o + size, z),
        ],
        &[],
    )
}

fn paint(scene: &mut Scene, f: FaceId, color: [f64; 3], mat: Option<&str>) {
    let attrs = &mut scene.mesh_mut().face_mut(f).attrs;
    attrs.paint.color = Some(color);
    attrs.paint.mat = mat.map(str::to_string);
}

// ---- Basic counts ---------------------------------------------------------

#[test]
fn test_empty_scene() {
    let stats = collect_stats(&scene());
    assert_eq!(stats.total_verts, 0);
    assert_eq!(stats.bbox.width, 0.0);
    assert!(stats.colors.is_empty());
}

#[test]
fn test_geometry_and_groups() {
    let mut scene = scene();
    quad(scene.mesh_mut(), 0.0, 0.0, 1.0);
    let mut g_mesh = Mesh::new();
    g_mesh.add_face(&[v3(2., 0., 0.), v3(3., 0., 0.), v3(2.5, 1., 0.)], &[]);
    let id = add_mesh(&mut scene.doc.meshes, g_mesh);
    scene.doc.groups.push(Group::new(id, Some("TestGroup")));

    let stats = collect_stats(&scene);
    assert!(stats.loose_verts == 4 && stats.loose_faces == 1);
    assert!(stats.group_verts == 3 && stats.group_faces == 1);
    assert!(stats.total_verts == 7 && stats.total_faces == 2);
    assert_eq!(stats.tri_count, 3); // quad -> 2, triangle -> 1
    assert!((stats.bbox.width - 3.0).abs() < 1e-6);
}

/// Model Info is a plain menu entry: Window ▸ Model Info, with no key, that opens
/// the dialog and leaves the tool in hand alone.
#[test]
fn test_tool_metadata() {
    let mut w = common::window();
    let a = w.actions.get("model_info").expect("the Model Info action");
    assert_eq!(a.text, "Model Info");
    assert!(a.shortcuts.is_empty() && !a.checkable);
    let (_, window_menu) = w
        .menus
        .iter()
        .find(|(t, _)| t == "Window")
        .expect("the Window menu");
    assert!(window_menu
        .iter()
        .any(|i| matches!(i, MenuItem::Action(id) if id == "model_info")));

    w.activate_tool("line");
    assert!(w.trigger("model_info"));
    assert!(w.model_info_requested);
    assert_eq!(w.active_tool_key(), Some("line"));
    // the window remembers it: Shift+R opens it again
    assert_eq!(
        w.last_command().map(|(label, _)| label.as_str()),
        Some("Model Info")
    );
}

// ---- The four numbers the original contribution got wrong ------------------

/// The original read attrs["material"], which nothing in Xtrude CAD writes:
/// its Materials tab was empty no matter how painted the model was. The
/// real key is the face colour (floats 0–1).
#[test]
fn test_materials_read_the_attrs_xtrude_writes() {
    let mut scene = scene();
    let f1 = quad(scene.mesh_mut(), 0.0, 0.0, 4.0); // 16 m², painted red
    paint(&mut scene, f1, [0.8, 0.1, 0.1], None);
    let f2 = quad(scene.mesh_mut(), 1.0, 0.0, 4.0); // 16 m², same red
    paint(&mut scene, f2, [0.8, 0.1, 0.1], None);

    let stats = collect_stats(&scene);
    assert_eq!(stats.colors.len(), 1);
    let red = &stats.colors[0];
    assert!(red.faces == 2 && (red.area - 32.0).abs() < 1e-6);
    assert_eq!(red.rgb, [0.8, 0.1, 0.1]); // floats kept, not int-cast
}

/// A named colour reads as ONE takeoff line — 'Matte grey: N faces,
/// X m²' — in stats and text.
#[test]
fn test_named_colors_group_under_their_registry_name() {
    let mut scene = scene();
    for z in [0.0, 1.0, 2.0] {
        let f = quad(scene.mesh_mut(), z, 0.0, 4.0);
        paint(&mut scene, f, [0.62, 0.60, 0.58], Some("Matte grey"));
    }
    let f2 = quad(scene.mesh_mut(), 3.0, 0.0, 2.0); // anonymous red, 4 m²
    paint(&mut scene, f2, [0.8, 0.1, 0.1], None);

    let stats = collect_stats(&scene);
    let named: Vec<_> = stats.colors.iter().filter(|c| c.mat.is_some()).collect();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].mat.as_deref(), Some("Matte grey"));
    assert_eq!(named[0].faces, 3);
    assert!((named[0].area - 3.0 * 16.0).abs() < 1e-6); // aggregated m²

    let text = stats_to_text(&stats);
    assert!(text.contains("Matte grey: 3")); // the takeoff line
    assert!(!text.contains("rgb(158,153,148)")); // name replaces rgb
    assert!(text.contains("rgb(204,26,26)")); // anonymous keeps rgb
}

/// Entering a group makes its mesh the active one; reading that
/// double-counted (the original reported 24 vertices for this 16-vertex
/// model). The loose mesh is the honest source either way.
#[test]
fn test_totals_stable_while_a_group_is_open() {
    let mut scene = scene();
    quad(scene.mesh_mut(), 0.0, 0.0, 4.0);
    let mut g_mesh = Mesh::new();
    for i in 0..3 {
        quad(&mut g_mesh, i as f64 * 10.0, 20.0, 4.0);
    }
    let id = add_mesh(&mut scene.doc.meshes, g_mesh);
    let g = Group::new(id, Some("Enclosure"));
    let gid = g.id;
    scene.doc.groups.push(g);

    let closed = collect_stats(&scene);
    scene.begin_group_edit(gid);
    let opened = collect_stats(&scene);
    scene.end_group_edit();

    assert_eq!(closed.total_verts, 16);
    assert_eq!(opened.total_verts, closed.total_verts);
    assert_eq!(opened.total_faces, closed.total_faces);
    assert_eq!(opened.loose_verts, closed.loose_verts);
}

// ---- Formatting -----------------------------------------------------------

#[test]
fn test_lengths_follow_the_dimension_style() {
    let mut scene = scene();
    quad(scene.mesh_mut(), 0.0, 0.0, 2.5);
    scene.dimension_style.units = "cm".into();
    scene.dimension_style.decimals = 1;
    let text = stats_to_text(&collect_stats(&scene));
    assert!(text.contains("250.0 cm"), "{text}"); // 2.5 m in the document's unit
}

#[test]
fn test_stats_to_text_mentions_the_essentials() {
    let mut scene = scene();
    let f = quad(scene.mesh_mut(), 0.0, 0.0, 4.0);
    paint(&mut scene, f, [1.0, 0.0, 0.0], None);
    let text = stats_to_text(&collect_stats(&scene));
    assert!(text.contains("rgb(255,0,0)"));
    assert!(text.contains("Layer 0"));
}
