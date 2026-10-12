// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What Move / Rotate drag is left out of the snap candidates — no "self
//! inference".

use crate::common;

use common::{sized_viewport, stub, v3};
use glam::DVec3;
use std::collections::HashSet;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::{EdgeId, FaceId};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn square(vp: &mut Viewport, x: f64) -> FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v(x, 0.), v(x + 1., 0.), v(x + 1., 1.), v(x, 1.)], &[])
}

fn select(vp: &mut Viewport, f: FaceId) {
    vp.scene.clear_selection();
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
}

fn edges_of(vp: &Viewport, f: FaceId) -> HashSet<EdgeId> {
    let m = vp.scene.mesh();
    let verts: HashSet<_> = m.face(f).outer.iter().copied().collect();
    m.edge_ids()
        .into_iter()
        .filter(|&e| {
            let ed = m.edge(e);
            verts.contains(&ed.v0) || verts.contains(&ed.v1)
        })
        .collect()
}

#[test]
fn test_move_names_the_edges_it_drags() {
    let mut vp = stub();
    let moving = square(&mut vp, 0.);
    let still = square(&mut vp, 3.);
    select(&mut vp, moving);
    let mut t = MoveTool::new();
    assert!(t.snap_excluded(&vp).is_none()); // nothing in motion yet
    t.on_click(&mut vp, &ToolContext::at(v(0., 0.)));
    let ex = t.snap_excluded(&vp).unwrap();
    assert!(ex.groups.is_empty());
    assert_eq!(ex.edges, edges_of(&vp, moving));
    assert!(edges_of(&vp, still).is_disjoint(&ex.edges));
}

#[test]
fn test_a_copy_in_progress_excludes_nothing() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ToolContext::at(v(0., 0.)));
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    assert!(t.snap_excluded(&vp).is_none()); // the original stays put
}

#[test]
fn test_rotate_names_the_geometry_swinging() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    select(&mut vp, f);
    let mut t = RotateTool::new();
    t.on_click(&mut vp, &ToolContext::at(v(0., 0.))); // centre
    assert!(t.snap_excluded(&vp).is_none()); // not swinging yet
    t.on_click(&mut vp, &ToolContext::at(v(1., 0.))); // base arm
    assert_eq!(t.snap_excluded(&vp).unwrap().edges.len(), 4);
}

#[test]
fn test_the_viewport_drops_the_moving_edges_from_the_snap_scene() {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v(2., 0.5);
    vp.camera.distance = 12.0;
    let moving = square(&mut vp, 0.);
    square(&mut vp, 3.);
    vp.scene.version += 1;
    select(&mut vp, moving);
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    let px = vp.world_to_pixel(v3(3., 0., 0.)).unwrap();
    let mesh_edges = |vp: &mut Viewport| -> Vec<(DVec3, DVec3)> {
        vp.snap_scene(Some(px.tuple()))
            .into_iter()
            .filter(|e| !e.center && (e.b - e.a).length() > 1e-9)
            .map(|e| (e.a, e.b))
            .collect()
    };
    assert_eq!(mesh_edges(&mut vp).len(), 8); // both squares
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(DVec3::ZERO)));
    let during = mesh_edges(&mut vp);
    assert_eq!(during.len(), 4);
    assert!(during
        .iter()
        .all(|(a, b)| a.x >= 3.0 - 1e-9 && b.x >= 3.0 - 1e-9));
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert_eq!(mesh_edges(&mut vp).len(), 8);
}
