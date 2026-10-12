// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A copied edge laid across a face splits it (copies go through the Line
//! tool's planner), and a face copied with its edges is not doubled.

use crate::common;

use common::{draw_rect, stub};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn copy(vp: &mut Viewport, grab: (f64, f64), drop: (f64, f64)) {
    let mut t = MoveTool::new();
    t.on_click(vp, &ToolContext::at(v(grab.0, grab.1)));
    t.on_key(vp, Key::Control, Modifiers::NONE);
    t.on_hover(vp, &ToolContext::at(v(drop.0, drop.1)));
    t.on_click(vp, &ToolContext::at(v(drop.0, drop.1)));
}

fn select_edge(vp: &mut Viewport, a: DVec3, b: DVec3) {
    let e = vp.scene.mesh_mut().add_edge(a, b).unwrap();
    vp.scene.selection.insert(EntityRef::Edge(MeshId::LOOSE, e));
}

fn faces(vp: &Viewport) -> usize {
    vp.scene.mesh().face_ids().len()
}

#[test]
fn test_a_copied_edge_across_a_face_splits_it() {
    let mut vp = stub();
    draw_rect(&mut vp, &[v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)]);
    select_edge(&mut vp, v(6., 0.), v(6., 4.)); // beside the square
    assert_eq!(faces(&vp), 1);
    copy(&mut vp, (6., 0.), (2., 0.)); // lay the copy at x = 2
    assert_eq!(faces(&vp), 2); // the square is split
    let m = vp.scene.mesh();
    let mut areas: Vec<i64> = m
        .face_ids()
        .iter()
        .map(|&f| (m.face_area(f) * 1e6).round() as i64)
        .collect();
    areas.sort();
    assert_eq!(areas, vec![8_000_000, 8_000_000]);
    vp.undo(); // one step back
    assert_eq!(faces(&vp), 1);
}

#[test]
fn test_a_copied_edge_crossing_past_the_face_splits_it_too() {
    let mut vp = stub();
    draw_rect(&mut vp, &[v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)]);
    select_edge(&mut vp, v(6., -1.), v(6., 5.));
    copy(&mut vp, (6., -1.), (2., -1.));
    assert_eq!(faces(&vp), 2);
}

#[test]
fn test_copying_a_face_with_its_edges_does_not_double_it() {
    let mut vp = stub();
    draw_rect(&mut vp, &[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)]);
    let m = vp.scene.mesh();
    let (fs, es) = (m.face_ids(), m.edge_ids());
    for f in fs {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    for e in es {
        vp.scene.selection.insert(EntityRef::Edge(MeshId::LOOSE, e));
    }
    copy(&mut vp, (0., 0.), (5., 0.));
    assert_eq!(faces(&vp), 2); // the original + ONE copy
    assert_eq!(vp.scene.mesh().edge_ids().len(), 8);
}
