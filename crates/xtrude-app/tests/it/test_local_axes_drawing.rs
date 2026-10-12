// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! App tests: inside a group you draw on ITS axes — the
//! plane lock, the ground, the arrow locks and the drawn axes follow the open
//! context, and the viewport syncs them as it enters and leaves, without
//! waiting for a pointer event. (The engine
//! half is in xtrude-core, the group-making half in xtrude-model.)

use crate::common;
use xtrude_core::Axis;

use common::sized_viewport;
use glam::{DMat4, DVec3};
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::viewport::render::push_axis;
use xtrude_app::viewport::Viewport;
use xtrude_core::axes;
use xtrude_core::math::{rotation_deg, VecExt};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;
use xtrude_render::frame::FrameData;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-4
}

fn turn(deg: f64) -> DMat4 {
    DMat4::from_translation(v(2., 1.)) * rotation_deg(DVec3::Z, deg)
}

fn red_30() -> DVec3 {
    let a = 30f64.to_radians();
    v(a.cos(), a.sin())
}

/// The drawing axes back to the world's when a test ends (they are
/// thread-local state).
struct WorldAxes;

impl Drop for WorldAxes {
    fn drop(&mut self) {
        axes::sync(None);
    }
}

#[test]
fn test_the_world_frame_is_bit_for_bit_the_old_tables() {
    // The snap engine and the plane lock both read `axes::axis` (there is
    // no other table), so the values are checked.
    let _w = WorldAxes;
    axes::sync(None);
    let got: Vec<DVec3> = Axis::ALL.into_iter().map(axes::axis).collect();
    assert_eq!(got, vec![DVec3::X, DVec3::Y, DVec3::Z]);
    assert!(axes::is_world());
    let mut lock = xtrude_app::tools::PlaneLock::default();
    for a in Axis::ALL {
        lock.plane_lock = Some(a);
        assert_eq!(
            lock.locked_work_plane(DVec3::ZERO).unwrap().1,
            axes::axis(a)
        );
    }
}

#[test]
fn test_plane_lock_and_default_ground_follow_the_context() {
    let _w = WorldAxes;
    let frame = rotation_deg(DVec3::X, 90.0); // the group stands up
    axes::sync(Some(&frame));
    let mut tool = RectangleTool::new();
    tool.lock.plane_lock = Some(Axis::Z);
    let (_p, n) = tool.lock.locked_work_plane(v(0., 0.)).expect("locked");
    assert!(close(n, DVec3::new(0., -1., 0.)) || close(n, DVec3::new(0., 1., 0.))); // local blue
    tool.lock.plane_lock = None;
    tool.work_plane = None;
    tool.lock.hover_plane = None;
    let (_o, ground) = tool.drawing_plane();
    assert!(close(ground, axes::axis(Axis::Z)));
}

// ---- through the real viewport ------------------------------------------------

fn vp_in_turned_group() -> (Viewport, GroupId) {
    let mut vp = sized_viewport(1000., 700.);
    let mut m = Mesh::new();
    m.add_face(&[v(-1., -1.), v(1., -1.), v(1., 1.), v(-1., 1.)], &[]);
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some("turned"));
    g.axes = Some(turn(30.0));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.begin_group_edit(gid);
    vp.camera.set_view("top");
    vp.camera.target = v(2., 1.);
    vp.camera.distance = 12.0;
    (vp, gid)
}

#[test]
fn test_the_viewport_syncs_the_axes_as_it_enters_and_leaves() {
    let _w = WorldAxes;
    let (mut vp, _g) = vp_in_turned_group();
    vp.sync_axes();
    assert!(!axes::is_world());
    vp.end_group_edit();
    vp.sync_axes();
    assert!(axes::is_world());
}

#[test]
fn test_an_arrow_lock_inside_the_group_runs_along_its_red() {
    let _w = WorldAxes;
    let (mut vp, _g) = vp_in_turned_group();
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    vp.tool_mut::<LineTool>()
        .expect("the Line tool")
        .start_point = Some(v(2., 1.));
    vp.inference.axis_lock = Some(Axis::X);
    let px = vp.world_to_pixel(v(3.5, 3.0)).expect("on screen"); // well off the red
    vp.input.last_mouse_pos = Some(px);
    vp.refresh_snap();
    let s = vp.inference.last_snap.clone().expect("a snap");
    let d = s.point - v(2., 1.);
    assert!(d.length() > 0.1);
    let n = d.unit_or_zero();
    assert!(close(n, red_30()) || close(n, -red_30()));
}

#[test]
fn test_the_axes_are_drawn_at_the_groups_origin() {
    let _w = WorldAxes;
    let (vp, _g) = vp_in_turned_group();
    vp.sync_axes();
    let mut f = FrameData::default();
    push_axis(&mut f, &vp.camera, Axis::X, 0.1, [1.0; 4]);
    let p = |i: usize| {
        let q = f.lines[i].pos;
        DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)
    };
    let (o, tip) = (p(0), p(1));
    assert!(close(o, v(2., 1.)));
    assert!(close((tip - o).unit_or_zero(), red_30()));
}

// ---- no pointer event between the context change and the axes ----------------

/// A copy placed at (3, 2, 0), away from the world origin.
fn vp_with_placed_copy() -> (Viewport, GroupId) {
    let mut vp = sized_viewport(1000., 700.);
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some("placed"));
    g.xform = Some(DMat4::from_translation(v(3., 2.)));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    (vp, gid)
}

#[test]
fn test_the_axes_follow_entering_and_leaving_without_a_pointer_event() {
    let _w = WorldAxes;
    let (mut vp, gid) = vp_with_placed_copy();
    axes::sync(None);
    vp.begin_group_edit(gid);
    assert!(close(axes::origin(), v(3., 2.)), "{}", axes::origin());
    vp.end_group_edit();
    assert!(axes::is_world());
    assert_eq!(axes::origin(), DVec3::ZERO);
    vp.begin_group_edit(gid);
    vp.end_one_group_edit();
    assert!(axes::is_world());
}

#[test]
fn test_the_axes_follow_undo_and_redo_that_close_the_open_group() {
    let _w = WorldAxes;
    let (mut vp, gid) = vp_with_placed_copy();
    vp.run(xtrude_model::commands::DeleteGroupCommand { group: gid });
    assert!(vp.undo());
    vp.begin_group_edit(gid);
    assert!(!axes::is_world());
    assert!(vp.redo(), "the open group is deleted again");
    assert!(vp.scene.edit_group().is_none());
    assert!(axes::is_world());
}

#[test]
fn test_a_frame_draws_the_axes_of_the_current_context() {
    let _w = WorldAxes;
    let (mut vp, _gid) = vp_with_placed_copy();
    // Stale axes from a context that is gone: the frame syncs them first.
    axes::sync(Some(&DMat4::from_translation(v(3., 2.))));
    let _ = vp.frame_data([1000, 700], false);
    assert!(axes::is_world());
}
