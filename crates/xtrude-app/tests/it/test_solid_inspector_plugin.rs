// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Solid Inspector's diagnosis must agree with the engine.
//!
//! The verdicts and volumes come from the same primitives the Solids tools
//! use, so whatever this dialog says must match what they would do.

use crate::common;

use common::v3;
use indexmap::IndexSet;
use xtrude_app::window::actions::MenuItem;
use xtrude_app::window::solid_inspector::{inspect_mesh, SolidInspector};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;

/// An axis-aligned box with consistent outward windings (the floor is
/// clockwise seen from above).
fn boxed(mesh: &mut Mesh, o: [f64; 3], d: [f64; 3], skip_top: bool) -> Vec<FaceId> {
    let [x0, y0, z0] = o;
    let (x1, y1, z1) = (x0 + d[0], y0 + d[1], z0 + d[2]);
    let mut quads = vec![
        [
            v3(x0, y1, z0),
            v3(x1, y1, z0),
            v3(x1, y0, z0),
            v3(x0, y0, z0),
        ],
        [
            v3(x0, y0, z0),
            v3(x1, y0, z0),
            v3(x1, y0, z1),
            v3(x0, y0, z1),
        ],
        [
            v3(x1, y0, z0),
            v3(x1, y1, z0),
            v3(x1, y1, z1),
            v3(x1, y0, z1),
        ],
        [
            v3(x1, y1, z0),
            v3(x0, y1, z0),
            v3(x0, y1, z1),
            v3(x1, y1, z1),
        ],
        [
            v3(x0, y1, z0),
            v3(x0, y0, z0),
            v3(x0, y0, z1),
            v3(x0, y1, z1),
        ],
    ];
    if !skip_top {
        quads.push([
            v3(x0, y0, z1),
            v3(x1, y0, z1),
            v3(x1, y1, z1),
            v3(x0, y1, z1),
        ]);
    }
    quads.iter().map(|q| mesh.add_face(q, &[])).collect()
}

fn cube(mesh: &mut Mesh, skip_top: bool) -> Vec<FaceId> {
    boxed(mesh, [0.0; 3], [2.0; 3], skip_top)
}

fn names(dlg: &SolidInspector) -> Vec<String> {
    (0..dlg.rows.len())
        .map(|i| dlg.cells(i)[0].clone())
        .collect()
}

// ---- Diagnosis ------------------------------------------------------------

#[test]
fn test_closed_cube_is_watertight_with_volume() {
    let mut mesh = Mesh::new();
    cube(&mut mesh, false);
    let rep = inspect_mesh(&mesh);
    assert!(rep.watertight);
    assert!(rep.open.is_empty() && rep.stray.is_empty() && rep.over.is_empty());
    assert!((rep.volume.unwrap() - 8.0).abs() < 1e-6); // 2 × 2 × 2
}

/// A box without its top: the four rim edges are open borders — the exact
/// edges a user needs to see to close the hole.
#[test]
fn test_open_box_reports_the_rim() {
    let mut mesh = Mesh::new();
    cube(&mut mesh, true);
    let rep = inspect_mesh(&mesh);
    assert!(!rep.watertight);
    assert_eq!(rep.open.len(), 4);
    assert_eq!(rep.volume, None);
    for &e in &rep.open {
        let edge = mesh.edge(e);
        assert!((mesh.pos(edge.v0).z - 2.0).abs() < 1e-6); // all on the top rim
        assert!((mesh.pos(edge.v1).z - 2.0).abs() < 1e-6);
    }
}

#[test]
fn test_stray_edge_detected() {
    let mut mesh = Mesh::new();
    cube(&mut mesh, false);
    mesh.add_edge(v3(10., 10., 0.), v3(12., 10., 0.)).unwrap();
    let rep = inspect_mesh(&mesh);
    assert!(!rep.watertight);
    assert!(rep.stray.len() == 1 && rep.open.is_empty());
}

/// Two boxes sharing a wall: the shared edges see 3 faces — overconnected.
/// Marked interior (what core orient does), the wall stops counting and the
/// union is watertight again — same rule as the engine.
#[test]
fn test_internal_wall_is_overconnected_but_interior_flag_heals() {
    let mut mesh = Mesh::new();
    boxed(&mut mesh, [0.0; 3], [2.0; 3], false);
    boxed(&mut mesh, [2.0, 0.0, 0.0], [2.0; 3], false);
    let rep = inspect_mesh(&mesh);
    assert!(!rep.watertight && rep.over.len() == 4);

    // The wall between the boxes: the face whose 4 verts all sit at x == 2.
    let wall: Vec<FaceId> = mesh
        .faces
        .iter()
        .filter(|(_, f)| f.outer.iter().all(|&v| (mesh.pos(v).x - 2.0).abs() < 1e-6))
        .map(|(id, _)| id)
        .collect();
    for f in wall {
        mesh.face_mut(f).interior = true;
    }
    let rep2 = inspect_mesh(&mesh);
    assert!(rep2.watertight);
    assert!((rep2.volume.unwrap() - 16.0).abs() < 1e-6); // 4 × 2 × 2 shell
}

#[test]
fn test_empty_mesh_is_not_a_solid() {
    let rep = inspect_mesh(&Mesh::new());
    assert!(!rep.watertight && rep.volume.is_none());
}

// ---- Dialog ---------------------------------------------------------------

#[test]
fn test_dialog_rows_and_highlight() {
    let mut w = common::window();
    let scene = &mut w.viewport.scene;
    cube(scene.mesh_mut(), true); // broken loose solid
    let mut g_mesh = Mesh::new();
    cube(&mut g_mesh, false); // closed group
    let id = add_mesh(&mut scene.doc.meshes, g_mesh);
    let g = Group::new(id, Some("Box OK"));
    let gid = g.id;
    scene.doc.groups.push(g);

    let mut dlg = SolidInspector::new(scene);
    let names = names(&dlg);
    assert!(names[0] == "Loose geometry" && names.contains(&"Box OK".to_string()));

    // The group row shows its closed volume.
    let i_g = names.iter().position(|n| n == "Box OK").unwrap();
    assert!(dlg.cells(i_g)[4].contains("8.000"));

    // Highlighting the active row selects the 4 rim edges in the viewport.
    dlg.current = Some(0);
    dlg.highlight(&mut w.viewport);
    let scene = &w.viewport.scene;
    assert_eq!(scene.selection.len(), 4);
    assert!(scene.selection.iter().all(|e| matches!(
        e,
        EntityRef::Edge(m, e) if *m == MeshId::LOOSE && scene.mesh().edges.contains(*e)
    )));

    // Highlighting the group row selects the group itself.
    dlg.current = Some(i_g);
    dlg.highlight(&mut w.viewport);
    assert_eq!(
        w.viewport.scene.selection,
        IndexSet::from([EntityRef::Group(gid)])
    );
}

/// Inside a group the active row inspects THAT mesh, and the group's own
/// listing row is not duplicated.
#[test]
fn test_dialog_group_edit_context() {
    let mut w = common::window();
    let scene = &mut w.viewport.scene;
    let mut g_mesh = Mesh::new();
    cube(&mut g_mesh, true);
    let id = add_mesh(&mut scene.doc.meshes, g_mesh);
    let g = Group::new(id, Some("Enclosure"));
    let gid = g.id;
    scene.doc.groups.push(g);
    scene.begin_group_edit(gid);

    let dlg = SolidInspector::new(scene);
    let names = names(&dlg);
    assert_eq!(names[0], "Open group: Enclosure");
    assert_eq!(names.iter().filter(|n| *n == "Enclosure").count(), 0); // not listed twice
    assert!(dlg.cells(0)[3].contains("4 open borders"));
    scene.end_group_edit();
}

/// Solid Inspector is a plain menu entry: Window ▸ Solid Inspector, with no key. A second
/// call refreshes the open inspector instead of opening another.
#[test]
fn test_tool_metadata_and_singleton() {
    let mut w = common::window();
    let a = w
        .actions
        .get("solid_inspector")
        .expect("the Solid Inspector action");
    assert_eq!(a.text, "Solid Inspector");
    assert!(a.shortcuts.is_empty() && !a.checkable);
    let (_, window_menu) = w
        .menus
        .iter()
        .find(|(t, _)| t == "Window")
        .expect("the Window menu");
    assert!(window_menu
        .iter()
        .any(|i| matches!(i, MenuItem::Action(id) if id == "solid_inspector")));

    w.activate_tool("line");
    assert!(w.trigger("solid_inspector"));
    assert_eq!(w.active_tool_key(), Some("line"));
    let first = w.solid_inspector.as_mut().expect("the inspector opened");
    assert_eq!(first.rows.len(), 1);
    first.current = Some(0); // a mark only this instance carries

    // the model changed: the second call refreshes the SAME inspector
    cube(w.viewport.scene.mesh_mut(), true);
    assert!(w.trigger("solid_inspector"));
    let again = w.solid_inspector.as_ref().unwrap();
    assert_eq!(again.current, Some(0));
    assert_eq!(again.rows[0].report.open.len(), 4);
    assert_eq!(
        w.last_command().map(|(label, _)| label.as_str()),
        Some("Solid Inspector")
    );
}
