// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A group in motion offers no snap points of its own: no "Component
//! origin" and no "Arc midpoint", like its edges and box corners.
//! Moved live (with loose geometry, or by Scale) its origin would
//! follow the cursor and the move would snap to itself; previewed by the
//! viewport, the points would sit where nothing is drawn.

use crate::common;

use common::{sized_viewport, stub};
use glam::{DMat4, DVec3};
use xtrude_app::input::{Modifiers, ScreenPt};
use xtrude_app::tools::arc::ArcTool;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::SnapKind;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// One half-circle arc (0,0)→(2,0) over (1,1), drawn with the real tool.
fn arc_mesh() -> Mesh {
    let mut st = stub();
    let mut arc = ArcTool::new();
    for w in [v(0., 0.), v(2., 0.), v(1., 1.)] {
        arc.on_click(&mut st, &ToolContext::at(w));
    }
    st.scene.mesh().clone()
}

/// The arc as a group translated by (dx, dy): origin (dx, dy), arc
/// midpoint (dx + 1, dy + 1).
fn arc_group(vp: &mut Viewport, dx: f64, dy: f64) -> Group {
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, arc_mesh()), None);
    g.xform = Some(DMat4::from_translation(v(dx, dy)));
    g
}

fn place(vp: &mut Viewport, g: Group) -> GroupId {
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    id
}

/// 1000×600, looking down on (6, 4).
fn top_vp() -> Viewport {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v(6., 4.);
    vp.camera.distance = 12.0;
    vp
}

fn select(vp: &mut Viewport, refs: &[EntityRef]) {
    vp.scene.clear_selection();
    for r in refs {
        vp.scene.selection.insert(*r);
    }
}

/// The scene offers a point of this kind at `p` (the cursor over `p`).
fn offers(vp: &mut Viewport, p: DVec3, origin: bool) -> bool {
    let px = vp.world_to_pixel(p).expect("on screen");
    vp.snap_scene(Some(px.tuple())).iter().any(|e| {
        (if origin {
            e.component_origin
        } else {
            e.arc_midpoint
        }) && (e.a - p).length() < 1e-6
    })
}

fn offers_origin(vp: &mut Viewport, p: DVec3) -> bool {
    offers(vp, p, true)
}

fn offers_arc_midpoint(vp: &mut Viewport, p: DVec3) -> bool {
    offers(vp, p, false)
}

#[test]
fn test_a_group_moved_live_offers_no_points_of_its_own() {
    let mut vp = top_vp();
    let g = arc_group(&mut vp, 5., 4.);
    let moving = place(&mut vp, g);
    let g = arc_group(&mut vp, 8., 1.);
    place(&mut vp, g); // stays put
    let f = vp
        .scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    vp.scene.version += 1;
    // With loose geometry in the move, the group's matrix follows the cursor.
    select(
        &mut vp,
        &[EntityRef::Group(moving), EntityRef::Face(MeshId::LOOSE, f)],
    );
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    assert!(offers_origin(&mut vp, v(5., 4.)));
    assert!(offers_arc_midpoint(&mut vp, v(6., 5.)));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(5., 4.))));
    vp.with_tool(|t, vp| t.on_hover(vp, &ToolContext::at(v(5.5, 4.5))));
    let xf = vp.scene.doc.find_group(moving).and_then(|g| g.xform);
    assert_eq!(xf, Some(DMat4::from_translation(v(5.5, 4.5)))); // live
    assert!(!offers_origin(&mut vp, v(5.5, 4.5)));
    assert!(!offers_arc_midpoint(&mut vp, v(6.5, 5.5)));
    assert!(offers_origin(&mut vp, v(8., 1.)));
    assert!(offers_arc_midpoint(&mut vp, v(9., 2.)));
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert!(offers_origin(&mut vp, v(5., 4.)));
    assert!(offers_arc_midpoint(&mut vp, v(6., 5.)));
}

#[test]
fn test_a_move_grabbed_at_the_origin_follows_the_cursor() {
    // The feedback loop: the origin under the cursor held the move back.
    let mut vp = top_vp();
    let g = arc_group(&mut vp, 5., 4.);
    let moving = place(&mut vp, g);
    let f = vp
        .scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    vp.scene.version += 1;
    select(
        &mut vp,
        &[EntityRef::Group(moving), EntityRef::Face(MeshId::LOOSE, f)],
    );
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(5., 4.))));
    for x in [5.1, 5.2, 5.3] {
        let ScreenPt { x, y } = vp.world_to_pixel(v(x, 4.3)).expect("on screen");
        vp.process_hover(x, y, Modifiers::NONE);
        let snap = vp.inference.last_snap.clone().expect("a snap");
        assert_ne!(snap.kind, SnapKind::ComponentOrigin);
    }
}

#[test]
fn test_a_group_previewed_by_the_viewport_offers_no_points_of_its_own() {
    // Groups only: the viewport draws the move and the matrix stays at the
    // start until the commit; the points there belong to nothing drawn.
    let mut vp = top_vp();
    let g = arc_group(&mut vp, 5., 4.);
    let moving = place(&mut vp, g);
    select(&mut vp, &[EntityRef::Group(moving)]);
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(5., 4.))));
    vp.with_tool(|t, vp| t.on_hover(vp, &ToolContext::at(v(5.5, 4.5))));
    let xf = vp.scene.doc.find_group(moving).and_then(|g| g.xform);
    assert_eq!(xf, Some(DMat4::from_translation(v(5., 4.)))); // not live
    assert!(!offers_origin(&mut vp, v(5., 4.)));
    assert!(!offers_arc_midpoint(&mut vp, v(6., 5.)));
}

#[test]
fn test_a_nested_group_moves_with_its_parent() {
    let mut vp = top_vp();
    let child = arc_group(&mut vp, 1., 0.); // in the parent: origin (6, 4), arc midpoint (7, 5)
    let mut parent = Group::new(add_mesh(&mut vp.scene.doc.meshes, Mesh::new()), None);
    parent.xform = Some(DMat4::from_translation(v(5., 4.)));
    parent.adopt(vec![child]);
    let parent = place(&mut vp, parent);
    select(&mut vp, &[EntityRef::Group(parent)]);
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    assert!(offers_origin(&mut vp, v(6., 4.)));
    assert!(offers_arc_midpoint(&mut vp, v(7., 5.)));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(5., 4.))));
    assert!(!offers_origin(&mut vp, v(5., 4.)));
    assert!(!offers_origin(&mut vp, v(6., 4.)));
    assert!(!offers_arc_midpoint(&mut vp, v(7., 5.)));
}

#[test]
fn test_a_group_swinging_offers_no_points_of_its_own() {
    let mut vp = top_vp();
    let g = arc_group(&mut vp, 5., 4.);
    let turning = place(&mut vp, g);
    select(&mut vp, &[EntityRef::Group(turning)]);
    vp.set_active_tool(Some(Box::new(RotateTool::new())));
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(4., 3.)))); // centre
    assert!(offers_origin(&mut vp, v(5., 4.))); // not swinging yet
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(v(5., 3.)))); // base arm
    assert!(!offers_origin(&mut vp, v(5., 4.)));
    assert!(!offers_arc_midpoint(&mut vp, v(6., 5.)));
}
