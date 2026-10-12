// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A planar shape started on a free point lies on the plane of the last hit;
//! the cursor glyph shows the plane; component faces give their world plane;
//! Esc steps out of groups; Paste and inserts follow the open context.

use crate::common;
use xtrude_core::Axis;

use common::{sized_viewport, v3, MetreDocument};
use glam::{DMat4, DVec3};
use xtrude_app::input::{Button, Key, MouseEvent};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::paste::PasteTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{plane_lock_axis, Tool, ToolContext, PREVIEW_PX};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::math::rotation_deg;
use xtrude_core::Mesh;
use xtrude_io::clip::ClipData;
use xtrude_model::commands::InsertGroupCommand;
use xtrude_model::group::{add_mesh, Group};

fn vp() -> Viewport {
    sized_viewport(1000., 600.)
}

/// A camera at eye level: 1.6 m up, looking level.
fn eye_level(vp: &mut Viewport, yaw_deg: f64) {
    let c = &mut vp.camera;
    c.target = v3(0., 0., 1.6);
    c.distance = 12.0;
    c.pitch = 0.0;
    c.yaw = yaw_deg.to_radians();
}

fn ctx(world: DVec3, px: (f64, f64)) -> ToolContext {
    let mut c = ToolContext::at(world);
    c.screen = px;
    c
}

fn hover_at(vp: &mut Viewport, px: (f64, f64)) -> DVec3 {
    let world = vp
        .world_from_pixel(px.0.trunc(), px.1.trunc())
        .expect("a world point");
    let c = ctx(world, px);
    vp.with_tool(|t, vp| t.on_hover(vp, &c));
    world
}

fn start_at_origin(vp: &mut Viewport, tool: Box<dyn Tool>) -> (f64, f64) {
    vp.set_active_tool(Some(tool));
    let o = vp.world_to_pixel(DVec3::ZERO).unwrap();
    let world = vp.world_from_pixel(o.x.trunc(), o.y.trunc()).unwrap();
    let c = ctx(world, o.tuple());
    vp.with_tool(|t, vp| t.on_click(vp, &c));
    o.tuple()
}

fn span(points: &[DVec3]) -> DVec3 {
    let lo = points
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = points
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    hi - lo
}

fn rect_corners(vp: &Viewport) -> Vec<DVec3> {
    let t = vp.tool::<RectangleTool>().unwrap();
    t.corners(t.start_point.unwrap(), t.hover_point.unwrap())
}

#[test]
fn test_rectangle_at_eye_level_stands_up_with_two_real_sides() {
    let _m = MetreDocument::new();
    let mut vp = vp();
    eye_level(&mut vp, 20.0);
    let o = start_at_origin(&mut vp, Box::new(RectangleTool::new()));
    assert!(vp.tool::<RectangleTool>().unwrap().work_plane.is_none()); // free point
    let horizon_y = vp.world_to_pixel(v3(0., 0., 1.6)).unwrap().y;
    // left of the start, just above the horizon
    hover_at(&mut vp, (o.0 - 250., horizon_y - 34.));
    let (text, _) = vp
        .tool::<RectangleTool>()
        .unwrap()
        .value_label(&vp)
        .unwrap();
    let wh: Vec<f64> = text
        .replace(" m", "")
        .split(" × ")
        .take(2)
        .map(|t| t.trim().parse().unwrap())
        .collect();
    assert!(wh[0] > 1.0 && wh[1] > 1.0, "{text}"); // not «5.74 × 0.00»
    let s = span(&rect_corners(&vp));
    assert!(s.z > 1.0); // it stands up
    assert!(s.x.min(s.y) < 1e-6); // in ONE vertical plane
}

#[test]
fn test_orbiting_after_the_first_click_re_decides_the_plane() {
    let mut vp = vp();
    eye_level(&mut vp, 20.0);
    let o = start_at_origin(&mut vp, Box::new(RectangleTool::new()));
    let horizon_y = vp.world_to_pixel(v3(0., 0., 1.6)).unwrap().y;
    hover_at(&mut vp, (o.0 - 250., horizon_y + 40.));
    assert!(span(&rect_corners(&vp)).z > 0.5);
    vp.camera.set_view("iso"); // orbit up
    let o = vp.world_to_pixel(DVec3::ZERO).unwrap();
    hover_at(&mut vp, (o.x - 120., o.y - 60.));
    let s = span(&rect_corners(&vp));
    assert!(s.z < 1e-6); // flat on the ground again
    assert!(s.x > 0.5 && s.y > 0.1);
}

#[test]
fn test_circle_at_eye_level_stands_up() {
    let mut vp = vp();
    eye_level(&mut vp, 80.0);
    let o = start_at_origin(&mut vp, Box::new(RadialTool::circle()));
    let horizon_y = vp.world_to_pixel(v3(0., 0., 1.6)).unwrap().y;
    hover_at(&mut vp, (o.0 - 150., horizon_y - 20.));
    let t = vp.tool::<RadialTool>().unwrap();
    let pts = t.points(t.start_point.unwrap(), t.hover_point.unwrap());
    let s = span(&pts);
    assert!(s.z > 1.0 && s.x.min(s.y) < 1e-6);
}

#[test]
fn test_a_captured_slab_keeps_a_planar_tool_flat_at_the_horizon() {
    let mut vp = vp();
    eye_level(&mut vp, 20.0);
    let mut tool = RectangleTool::new();
    tool.start_point = Some(DVec3::ZERO);
    tool.work_plane = Some((DVec3::ZERO, DVec3::Z)); // clicked a slab
    vp.set_active_tool(Some(Box::new(tool)));
    {
        let t = vp.tool_mut::<RectangleTool>().unwrap();
        t.start_point = Some(DVec3::ZERO);
        t.work_plane = Some((DVec3::ZERO, DVec3::Z));
    }
    let (_p, n) = vp.current_work_plane(None);
    assert!(n.z.abs() > 0.99);
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    {
        let t = vp.tool_mut::<LineTool>().unwrap();
        t.start_point = Some(DVec3::ZERO);
        t.work_plane = Some((DVec3::ZERO, DVec3::Z));
    }
    let (_p, n) = vp.current_work_plane(None);
    assert!(n.z.abs() < 1e-6); // still rises for a line
}

fn key_for(axis: Axis) -> Key {
    [Key::Right, Key::Left, Key::Up]
        .into_iter()
        .find(|k| plane_lock_axis(*k) == Some(axis))
        .unwrap()
}

fn rgb(c: Option<[f64; 4]>) -> Option<[f64; 3]> {
    c.map(|c| [c[0], c[1], c[2]])
}

#[test]
fn test_circle_shows_a_ring_on_the_cursor_in_the_locked_plane_colour() {
    let mut vp = vp();
    vp.camera.set_view("iso");
    vp.set_active_tool(Some(Box::new(RadialTool::circle())));
    let k = key_for(Axis::Y); // Left = green
    vp.with_tool(|t, vp| t.on_key(vp, k, Default::default()));
    let px = (500., 300.);
    let world = hover_at(&mut vp, px);
    let t = vp.tool::<RadialTool>().unwrap();
    let segs = t.rubber_band_lines(&vp);
    assert_eq!(segs.len(), t.sides); // a ring, before any click
    let s = span(&segs.iter().map(|(a, _)| *a).collect::<Vec<_>>());
    assert!(s.y < 1e-6 && s.x > 0. && s.z > 0.); // normal to green
    assert_eq!(rgb(t.wireframe_color()), Some(Axis::Y.color()));
    let r = vp.world_to_pixel(segs[0].0).unwrap();
    let c = vp.world_to_pixel(world).unwrap();
    assert!(((r.x - c.x).hypot(r.y - c.y) - PREVIEW_PX).abs() < 4.0);
    vp.with_tool(|t, vp| t.on_key(vp, k, Default::default())); // the same key frees it
    hover_at(&mut vp, px);
    let t = vp.tool::<RadialTool>().unwrap();
    assert!(t.wireframe_color().is_none());
    let s = span(
        &t.rubber_band_lines(&vp)
            .iter()
            .map(|(a, _)| *a)
            .collect::<Vec<_>>(),
    );
    assert!(s.z < 1e-6); // flat on the ground
}

fn rect_glyph_span(vp: &Viewport) -> DVec3 {
    let t = vp.tool::<RectangleTool>().unwrap();
    span(
        &t.rubber_band_lines(vp)
            .iter()
            .map(|(a, _)| *a)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn test_rectangle_shows_a_square_on_the_cursor_and_keeps_the_colour_while_drawing() {
    let mut vp = vp();
    vp.camera.set_view("iso");
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
    let px = (500., 300.);
    let world = hover_at(&mut vp, px);
    assert_eq!(
        vp.tool::<RectangleTool>()
            .unwrap()
            .rubber_band_lines(&vp)
            .len(),
        4
    );
    assert!(rect_glyph_span(&vp).z < 1e-6); // a flat square glyph
    assert!(vp
        .tool::<RectangleTool>()
        .unwrap()
        .wireframe_color()
        .is_none());
    let k = key_for(Axis::X); // Right = red
    vp.with_tool(|t, vp| t.on_key(vp, k, Default::default()));
    hover_at(&mut vp, px);
    assert!(rect_glyph_span(&vp).x < 1e-6); // normal to red
    let red = vp.tool::<RectangleTool>().unwrap().wireframe_color();
    let c = ctx(world, px);
    vp.with_tool(|t, vp| t.on_click(vp, &c));
    let c2 = ctx(world + v3(0., 1., 1.), px);
    vp.with_tool(|t, vp| t.on_hover(vp, &c2));
    assert_eq!(vp.tool::<RectangleTool>().unwrap().wireframe_color(), red); // still red
    assert!(rect_glyph_span(&vp).x < 1e-6);
    let t = vp.tool_mut::<RectangleTool>().unwrap();
    t.reset();
    assert!(t.wireframe_color().is_none() && t.lock.plane_lock.is_none());
}

#[test]
fn test_a_zero_sided_rectangle_is_refused_instead_of_raising() {
    let mut vp = vp();
    vp.camera.set_view("iso");
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
    let before = vp.scene.mesh().edges.len();
    let a = ctx(DVec3::ZERO, (0., 0.));
    let b = ctx(v3(3., 0., 0.), (0., 0.)); // same row: no height
    vp.with_tool(|t, vp| t.on_click(vp, &a));
    vp.with_tool(|t, vp| t.on_click(vp, &b));
    assert_eq!(vp.scene.mesh().edges.len(), before);
    let said: Vec<String> = vp
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect();
    assert!(said.last().is_some_and(|t| t.contains("two sides")));
}

fn post_mesh() -> Mesh {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(0., 2., 0.),
            v3(0., 2., 3.),
            v3(0., 0., 3.),
        ],
        &[],
    );
    m
}

fn press_at(vp: &mut Viewport, px: (f64, f64)) {
    vp.mouse_press(&MouseEvent::button(px.0, px.1, Button::Left, true));
}

#[test]
fn test_a_first_click_on_a_components_face_captures_its_world_plane() {
    let mut vp = vp();
    vp.camera.set_view("iso");
    let mid = add_mesh(&mut vp.scene.doc.meshes, post_mesh());
    let mut g = Group::new(mid, Some("peg"));
    g.xform = Some(DMat4::from_translation(v3(8., 5., 0.)));
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    let c = &mut vp.camera;
    c.target = v3(8., 6., 1.5);
    c.distance = 10.0;
    c.yaw = 180f64.to_radians(); // looking along -X at the face
    c.pitch = 10f64.to_radians();
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
    let px = vp.world_to_pixel(v3(8., 6., 1.5)).unwrap();
    press_at(&mut vp, px.tuple());
    let t = vp.tool::<RectangleTool>().unwrap();
    assert!(t.start_point.is_some());
    let (pt, n) = t.work_plane.expect("the post's plane");
    assert!((n.x.abs() - 1.0).abs() < 1e-6);
    assert!((pt.x - 8.0).abs() < 1e-6); // in WORLD space
}

#[test]
fn test_a_nested_components_face_plane_comes_through_its_placement() {
    let mut vp = vp();
    let mid = add_mesh(&mut vp.scene.doc.meshes, post_mesh());
    let mut child = Group::new(mid, Some("peg"));
    child.xform = Some(DMat4::from_translation(v3(20., 10., 0.)) * rotation_deg(DVec3::Z, 90.0));
    let cm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut container = Group::new(cm, Some("plaza"));
    container.xform = Some(DMat4::IDENTITY);
    container.children = vec![child];
    let cid = container.id;
    vp.scene.doc.groups.push(container);
    vp.scene.version += 1;
    let c = &mut vp.camera;
    c.target = v3(20., 11., 1.5);
    c.distance = 10.0;
    c.yaw = 90f64.to_radians(); // looking along -Y at the face
    c.pitch = 10f64.to_radians();
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
    let px = vp.world_to_pixel(v3(20., 11., 1.5)).unwrap();
    let hit = vp.pick_face_any(px.x, px.y).expect("a face");
    assert_eq!(hit.owner, Some(cid));
    let place = vp.pick_face_placement(px.x, px.y).expect("a placement");
    assert!(place.face == hit.face && place.placement != Some(cid));
    let (_p, n) = vp.face_plane_world(&place);
    assert!((n.y.abs() - 1.0).abs() < 1e-6 && n.x.abs() < 1e-6); // rotated into the world
    press_at(&mut vp, px.tuple());
    let (pt, n) = vp.tool::<RectangleTool>().unwrap().work_plane.unwrap();
    assert!((n.y.abs() - 1.0).abs() < 1e-6);
    assert!((pt.y - 10.0).abs() < 1e-6); // on the post, in the world
}

#[test]
fn test_opening_a_container_lets_the_pick_resolve_to_its_children() {
    let mut vp = vp();
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut child = Group::new(mid, Some("hinge"));
    child.xform = Some(DMat4::from_translation(v3(30., 30., 0.)));
    let chid = child.id;
    let cm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut container = Group::new(cm, Some("plaza"));
    container.xform = Some(DMat4::IDENTITY);
    container.children = vec![child];
    let cid = container.id;
    vp.scene.doc.groups.push(container);
    vp.scene.version += 1;
    vp.camera.set_view("top");
    vp.camera.target = v3(31., 31., 0.);
    vp.camera.distance = 10.0;
    let px = vp.world_to_pixel(v3(31., 31., 0.)).unwrap();
    assert_eq!(vp.pick_group(px.x, px.y), Some(cid));
    vp.scene.begin_group_edit(cid); // double-click: open it
    assert_eq!(vp.pick_group(px.x, px.y), Some(chid));
    vp.scene.end_group_edit();
}

#[test]
fn test_escape_steps_out_of_the_open_group_from_the_window_action_too() {
    let mut vp = vp();
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let child = Group::new(mid, Some("letters"));
    let chid = child.id;
    let cm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut container = Group::new(cm, Some("hinge"));
    container.children = vec![child];
    let cid = container.id;
    vp.scene.doc.groups.push(container);
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(SelectTool::new())));
    vp.scene.begin_group_edit(cid);
    vp.scene.begin_group_edit(chid);
    assert_eq!(vp.scene.edit_group(), Some(chid));
    vp.escape(); // what the window action calls
    assert_eq!(vp.scene.edit_group(), Some(cid)); // one level out…
    vp.escape();
    assert!(vp.scene.edit_group().is_none()); // …and out
}

#[test]
fn test_paste_places_on_the_plane_through_the_clipboard_reference() {
    let mut vp = vp();
    vp.camera.set_view("iso");
    vp.camera.target = v3(0., 0., 1.);
    vp.camera.distance = 12.0;
    vp.clipboard = Some(ClipData {
        reference: v3(0., 0., 1.),
        ..ClipData::default()
    });
    vp.set_active_tool(Some(Box::new(PasteTool::new())));
    assert_eq!(vp.tool_start_point(), Some(v3(0., 0., 1.)));
    let px = vp.world_to_pixel(v3(1., 0., 1.)).unwrap();
    let world = vp.world_from_pixel(px.x.trunc(), px.y.trunc()).unwrap();
    assert!((world.z - 1.0).abs() < 1e-6); // level with the slat, not z = 0
    assert!((world.x - 1.0).abs() < 0.05);
}

#[test]
fn test_a_group_inserted_while_editing_a_container_becomes_its_child() {
    let mut vp = vp();
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
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut child = Group::new(mid, Some("lid"));
    child.xform = Some(DMat4::IDENTITY);
    let cm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut container = Group::new(cm, Some("bracket"));
    container.xform = Some(DMat4::IDENTITY);
    container.children = vec![child];
    let cid = container.id;
    vp.scene.doc.groups.push(container);
    vp.scene.version += 1;
    let tops = vp.scene.doc.groups.len();
    vp.scene.begin_group_edit(cid);
    let mut copy = Group::new(mid, Some("lid"));
    copy.xform = Some(DMat4::from_translation(v3(0., 0., 3.)));
    let copy_id = copy.id;
    vp.run(InsertGroupCommand::new(copy));
    let kids = |vp: &Viewport| {
        vp.scene
            .doc
            .find_group(cid)
            .unwrap()
            .children
            .iter()
            .any(|c| c.id == copy_id)
    };
    assert!(kids(&vp) && !vp.scene.doc.groups.iter().any(|g| g.id == copy_id));
    assert_eq!(vp.scene.doc.groups.len(), tops);
    assert!(vp.undo());
    assert!(!kids(&vp));
    assert!(vp.redo());
    assert!(kids(&vp));
}
