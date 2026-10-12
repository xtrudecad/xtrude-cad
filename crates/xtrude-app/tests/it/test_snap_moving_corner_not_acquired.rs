// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! While a tool draws, the corner and the edge under the cursor become the
//! 'from point' and the acquired edge, but never those of what is in
//! motion: previewed by the viewport, a moving group's corner
//! sits where nothing is drawn; moved live, the corner follows the cursor
//! and the move snaps to itself.

use crate::common;

use common::sized_viewport;
use glam::{DMat4, DVec3};
use xtrude_app::input::Modifiers;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::ToolContext;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// The unit square at (dx, dy), as a group.
fn square_group(vp: &mut Viewport, dx: f64, dy: f64) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), None);
    g.xform = Some(DMat4::from_translation(v(dx, dy)));
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    id
}

/// A loose unit square at (dx, dy).
fn loose_square(vp: &mut Viewport, dx: f64, dy: f64) -> xtrude_core::FaceId {
    let f = vp.scene.mesh_mut().add_face(
        &[
            v(dx, dy),
            v(dx + 1., dy),
            v(dx + 1., dy + 1.),
            v(dx, dy + 1.),
        ],
        &[],
    );
    vp.scene.version += 1;
    f
}

/// 1000×600, looking down on (6, 4).
fn top_vp() -> Viewport {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v(6., 4.);
    vp.camera.distance = 12.0;
    vp
}

/// Start a move of `refs` grabbed at `from`.
fn start_move(vp: &mut Viewport, refs: &[EntityRef], from: DVec3) {
    vp.scene.clear_selection();
    for r in refs {
        vp.scene.selection.insert(*r);
    }
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(from)));
    assert!(vp.tool_start_point().is_some());
}

/// Hover over `p` with nothing acquired yet; what the hover acquires.
fn hover(vp: &mut Viewport, p: DVec3) -> (Option<DVec3>, Option<(DVec3, DVec3)>) {
    vp.inference.acquired_point = None;
    vp.inference.acquired_edge = None;
    let s = vp.world_to_pixel(p).expect("on screen");
    vp.process_hover(s.x, s.y, Modifiers::NONE);
    (vp.inference.acquired_point, vp.inference.acquired_edge)
}

fn near(a: Option<DVec3>, b: DVec3) -> bool {
    a.is_some_and(|a| (a - b).length() < 1e-6)
}

/// The edge `(a, b)`, either way round.
fn is_edge(e: Option<(DVec3, DVec3)>, a: DVec3, b: DVec3) -> bool {
    e.is_some_and(|(p, q)| {
        ((p - a).length() < 1e-6 && (q - b).length() < 1e-6)
            || ((p - b).length() < 1e-6 && (q - a).length() < 1e-6)
    })
}

/// The corners of the unit square at `at`.
fn corners(at: DVec3) -> [DVec3; 4] {
    [at, at + v(1., 0.), at + v(1., 1.), at + v(0., 1.)]
}

fn group_at(vp: &Viewport, g: GroupId) -> DVec3 {
    let xf = vp.scene.doc.find_group(g).and_then(|g| g.xform);
    xf.expect("placed").w_axis.truncate()
}

#[test]
fn test_a_group_previewed_by_the_viewport_lends_no_corner() {
    let mut vp = top_vp();
    let moving = square_group(&mut vp, 5., 4.);
    square_group(&mut vp, 8., 1.); // stays put
    start_move(&mut vp, &[EntityRef::Group(moving)], v(5.5, 4.5));
    // Over its corner at the start position, where nothing is drawn.
    for c in corners(v(5., 4.)) {
        let (point, _) = hover(&mut vp, c);
        assert!(!near(point, c), "corner {c} acquired");
        assert_eq!(group_at(&vp, moving), v(5., 4.)); // not live
    }
    assert!(near(hover(&mut vp, v(9., 2.)).0, v(9., 2.)));
}

#[test]
fn test_a_group_previewed_by_the_viewport_lends_no_edge() {
    let mut vp = top_vp();
    let moving = square_group(&mut vp, 5., 4.);
    square_group(&mut vp, 8., 1.);
    start_move(&mut vp, &[EntityRef::Group(moving)], v(5.5, 4.5));
    let (_, edge) = hover(&mut vp, v(5.5, 4.));
    assert!(
        !is_edge(edge, v(5., 4.), v(6., 4.)),
        "edge {edge:?} acquired"
    );
    let (_, edge) = hover(&mut vp, v(8.5, 1.));
    assert!(is_edge(edge, v(8., 1.), v(9., 1.)));
}

#[test]
fn test_a_group_moved_live_lends_no_corner() {
    // With loose geometry in the move, the group follows the cursor and its
    // grabbed corner stays under it.
    let mut vp = top_vp();
    let moving = square_group(&mut vp, 5., 4.);
    square_group(&mut vp, 8., 1.);
    let f = loose_square(&mut vp, 0., 0.);
    start_move(
        &mut vp,
        &[EntityRef::Group(moving), EntityRef::Face(MeshId::LOOSE, f)],
        v(5., 4.),
    );
    for p in [v(5.3, 4.3), v(5.3, 4.3), v(5.6, 4.2), v(5.6, 4.2)] {
        let (point, _) = hover(&mut vp, p);
        let at = group_at(&vp, moving);
        assert!((at - v(5., 4.)).length() > 0.1, "live"); // moved
        for c in corners(at) {
            assert!(!near(point, c), "corner {c} acquired");
        }
    }
    assert!(near(hover(&mut vp, v(9., 2.)).0, v(9., 2.)));
}

#[test]
fn test_a_group_moved_live_lends_no_edge() {
    let mut vp = top_vp();
    let moving = square_group(&mut vp, 5., 4.);
    let f = loose_square(&mut vp, 0., 0.);
    start_move(
        &mut vp,
        &[EntityRef::Group(moving), EntityRef::Face(MeshId::LOOSE, f)],
        v(5.5, 4.),
    );
    for p in [v(5.8, 4.3), v(5.8, 4.3)] {
        let (_, edge) = hover(&mut vp, p);
        let at = group_at(&vp, moving);
        assert!(!is_edge(edge, at, at + v(1., 0.)), "edge {edge:?} acquired");
    }
}

#[test]
fn test_loose_geometry_moved_lends_no_corner_or_edge() {
    let mut vp = top_vp();
    let f = loose_square(&mut vp, 5., 4.);
    loose_square(&mut vp, 8., 1.); // stays put
    start_move(&mut vp, &[EntityRef::Face(MeshId::LOOSE, f)], v(5., 4.));
    for p in [v(5.3, 4.3), v(5.3, 4.3), v(5.6, 4.2), v(5.6, 4.2)] {
        let (point, _) = hover(&mut vp, p);
        let mesh = vp.scene.mesh();
        let moved: Vec<DVec3> = mesh.faces[f].outer.iter().map(|&vx| mesh.pos(vx)).collect();
        assert!(moved.iter().any(|c| (*c - p).length() < 0.02), "live");
        for c in moved {
            assert!(!near(point, c), "corner {c} acquired");
        }
    }
    // Grabbed on an edge: the edge follows the cursor.
    vp.with_tool(|t, vp| t.on_cancel(vp));
    start_move(&mut vp, &[EntityRef::Face(MeshId::LOOSE, f)], v(5.5, 4.));
    for p in [v(5.8, 4.3), v(5.8, 4.3)] {
        let (_, edge) = hover(&mut vp, p);
        let a = p - v(0.5, 0.);
        assert!(!is_edge(edge, a, a + v(1., 0.)), "edge {edge:?} acquired");
    }
    // What stands still is still acquired.
    let (point, _) = hover(&mut vp, v(9., 2.));
    assert!(near(point, v(9., 2.)));
    let (_, edge) = hover(&mut vp, v(8.5, 1.));
    assert!(is_edge(edge, v(8., 1.), v(9., 1.)));
}
