// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every group keeps its OWN axes through Move/Rotate/Scale/Flip, copies, Make
//! Unique and Explode; the edit stack draws on each context's frame; the Scale
//! box sits on them; Change Axes moves nothing. The .igz and .skp tests
//! live in crates/xtrude-io.

use crate::common;

use common::{stub, v3};
use glam::{DMat4, DVec3};
use xtrude_app::tools::change_axes::ChangeAxesTool;
use xtrude_app::tools::scale::ScaleTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::axes::frame_axes;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_model::commands::{
    scale_matrix, ChangeAxesCommand, ExplodeGroupCommand, FlipGroupsCommand, MoveGroupCommand,
    RotateGroupCommand, ScaleFactor, ScaleGroupCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, group_frame, materialize, world_mesh, Group};
use xtrude_model::ids::GroupId;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-5
}

fn slab_mesh() -> Mesh {
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(2., 0.), v(2., 1.), v(0., 1.)], &[]);
    m
}

fn slab(vp: &mut Viewport) -> GroupId {
    let mid = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let g = Group::new(mid, Some("slab"));
    let id = g.id;
    vp.scene.doc.groups.push(g);
    id
}

fn frame(vp: &Viewport, g: GroupId) -> Option<DMat4> {
    group_frame(vp.scene.doc.find_group(g).unwrap())
}

fn red(vp: &Viewport, g: GroupId) -> DVec3 {
    frame_axes(frame(vp, g).as_ref()).1
}

fn rotate(vp: &mut Viewport, g: GroupId, deg: f64) {
    vp.run(RotateGroupCommand {
        group: g,
        center: v(0., 0.),
        axis: v3(0., 0., 1.),
        degrees: deg,
    });
}

fn rot_z(deg: f64) -> DMat4 {
    DMat4::from_rotation_z(deg.to_radians())
}

#[test]
fn test_a_new_group_has_the_world_axes() {
    let mut vp = stub();
    let g = slab(&mut vp);
    assert!(frame(&vp, g).is_none());
    let (_o, x, y, z) = frame_axes(None);
    assert_eq!((x, y, z), (v3(1., 0., 0.), v3(0., 1., 0.), v3(0., 0., 1.)));
}

#[test]
fn test_rotate_carries_the_axes_and_undo_brings_them_back() {
    let mut vp = stub();
    let g = slab(&mut vp);
    rotate(&mut vp, g, 30.0);
    let a = 30f64.to_radians();
    assert!(close(red(&vp, g), v(a.cos(), a.sin())));
    assert!(vp.undo());
    assert!(frame(&vp, g).is_none());
    assert!(vp.redo());
    assert!(close(red(&vp, g), v(a.cos(), a.sin())));
}

#[test]
fn test_move_carries_the_origin() {
    let mut vp = stub();
    let g = slab(&mut vp);
    rotate(&mut vp, g, 90.0);
    vp.run(MoveGroupCommand {
        group: g,
        delta: v3(5., 0., 1.),
    });
    let (o, x, _y, _z) = frame_axes(frame(&vp, g).as_ref());
    assert!(close(o, v3(5., 0., 1.)) && close(x, v(0., 1.)));
}

#[test]
fn test_scale_keeps_the_directions_unit() {
    let mut vp = stub();
    let g = slab(&mut vp);
    rotate(&mut vp, g, 45.0);
    vp.run(ScaleGroupCommand {
        group: g,
        center: v(0., 0.),
        factor: ScaleFactor::PerAxis([3.0, 1.0, 1.0]),
        axes: None,
    });
    let (_o, x, y, z) = frame_axes(frame(&vp, g).as_ref());
    for a in [x, y, z] {
        assert!((a.length() - 1.0).abs() < 1e-6);
    }
    assert!(x.dot(y).abs() < 1e-6);
}

#[test]
fn test_flip_mirrors_the_axes_and_flips_back() {
    let mut vp = stub();
    let g = slab(&mut vp);
    vp.run(FlipGroupsCommand {
        groups: vec![g],
        centre: v(0., 0.),
        axis: v3(1., 0., 0.),
    });
    assert!(close(red(&vp, g), v(-1., 0.)));
    assert!(vp.undo());
    assert!(close(red(&vp, g), v(1., 0.)));
}

#[test]
fn test_a_copy_takes_the_axes_along() {
    let mut vp = stub();
    let g = slab(&mut vp);
    vp.scene.doc.find_group_mut(g).unwrap().axes = Some(rot_z(30.0));
    let mut src = vp.scene.doc.find_group(g).unwrap().clone();
    let c = copy_group(&mut vp.scene.doc.meshes, &mut src, Some(v(10., 0.)), false);
    let (o, x, _y, _z) = frame_axes(group_frame(&c).as_ref());
    assert!(close(o, v(10., 0.)) && close(x, red(&vp, g)));
}

#[test]
fn test_make_independent_keeps_where_a_component_faced() {
    let mut vp = stub();
    let proto = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let mut inst = Group::new(proto, Some("comp"));
    let turn = DMat4::from_translation(v(4., 0.)) * rot_z(60.0);
    inst.xform = Some(turn);
    materialize(&mut vp.scene.doc.meshes, &mut inst);
    assert!(inst.xform.is_none() && inst.axes == Some(turn));
}

#[test]
fn test_explode_lifts_a_child_with_its_axes_composed() {
    let mut vp = stub();
    let cm = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let mut child = Group::new(cm, Some("slab"));
    let turn = rot_z(30.0);
    child.axes = Some(turn);
    let cid = child.id;
    let bm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut boxg = Group::new(bm, Some("box"));
    boxg.adopt(vec![child]);
    boxg.xform = Some(DMat4::from_translation(v(0., 5.)));
    let bid = boxg.id;
    vp.scene.doc.groups.push(boxg);
    vp.run(ExplodeGroupCommand { group: bid });
    let (o, x, _y, _z) = frame_axes(frame(&vp, cid).as_ref());
    assert!(close(o, v(0., 5.)) && close(x, frame_axes(Some(&turn)).1));
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.find_group(cid).unwrap().axes, Some(turn));
}

// ---- The edit stack reads each context's frame ---------------------------

#[test]
fn test_editing_a_turned_group_draws_on_its_axes() {
    let mut vp = stub();
    let g = slab(&mut vp);
    let turn = rot_z(30.0);
    vp.scene.doc.find_group_mut(g).unwrap().axes = Some(turn);
    assert!(vp.scene.drawing_frame().is_none());
    vp.scene.begin_group_edit(g);
    assert_eq!(vp.scene.drawing_frame(), Some(turn));
    vp.scene.end_group_edit();
    assert!(vp.scene.drawing_frame().is_none());
}

#[test]
fn test_editing_a_component_uses_its_placement_even_mid_edit() {
    let mut vp = stub();
    let proto = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let mut inst = Group::new(proto, Some("comp"));
    let turn = DMat4::from_translation(v(3., 0.)) * rot_z(45.0);
    inst.xform = Some(turn);
    let id = inst.id;
    vp.scene.doc.groups.push(inst);
    vp.scene.begin_group_edit(id);
    assert!(vp.scene.doc.find_group(id).unwrap().xform.is_none()); // a world copy
    assert_eq!(vp.scene.drawing_frame(), Some(turn)); // …the axes are its own
}

#[test]
fn test_nested_contexts_each_have_their_own_frame() {
    let mut vp = stub();
    let cm = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let mut child = Group::new(cm, Some("slab"));
    let inner = rot_z(30.0);
    child.axes = Some(inner);
    let cid = child.id;
    let bm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut boxg = Group::new(bm, Some("box"));
    boxg.adopt(vec![child]);
    let outer = DMat4::from_translation(v(0., 5.)) * rot_z(90.0);
    boxg.xform = Some(outer);
    let bid = boxg.id;
    vp.scene.doc.groups.push(boxg);
    vp.scene.begin_group_edit(bid);
    let (o, x, _y, _z) = frame_axes(vp.scene.drawing_frame().as_ref());
    assert!(close(o, v(0., 5.)) && close(x, v(0., 1.)));
    vp.scene.begin_group_edit(cid); // one level down
    let (_o, x2, _y, _z) = frame_axes(vp.scene.drawing_frame().as_ref());
    assert!(close(x2, frame_axes(Some(&(outer * inner))).1));
    vp.scene.end_one_group_edit();
    let (_o, x3, _y, _z) = frame_axes(vp.scene.drawing_frame().as_ref());
    assert!(close(x3, v(0., 1.))); // back to the container's
    vp.scene.end_group_edit();
    assert!(vp.scene.drawing_frame().is_none());
    // the container kept its axes after its matrix went down to the child
    assert!(close(red(&vp, bid), v(0., 1.)));
}

// ---- the Scale box sits on the object's axes ---------------------

#[test]
fn test_the_scale_box_of_a_turned_group_is_its_own_size() {
    let mut vp = stub();
    let g = slab(&mut vp); // 2 x 1
    rotate(&mut vp, g, 30.0);
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut tool = ScaleTool::new();
    let (lo, hi) = tool.selection_bounds(&vp).unwrap();
    let ext = hi - lo;
    assert!((ext.x - 2.0).abs() < 1e-5); // not the world AABB
    assert!((ext.y - 1.0).abs() < 1e-5);
}

#[test]
fn test_scaling_along_a_turned_red_stretches_only_that_way() {
    let a = 30f64.to_radians();
    let x = v(a.cos(), a.sin());
    let y = v(-a.sin(), a.cos());
    let z = v3(0., 0., 1.);
    let m = scale_matrix(
        v(0., 0.),
        ScaleFactor::PerAxis([2.0, 1.0, 1.0]),
        Some((x, y, z)),
    );
    assert!(close(m.project_point(x), x * 2.0)); // along red: doubled
    assert!(close(m.project_point(y), y)); // along green: untouched
}

#[test]
fn test_scale_group_command_along_its_axes_keeps_the_width() {
    let mut vp = stub();
    let g = slab(&mut vp); // 2 along x, 1 along y
    rotate(&mut vp, g, 30.0);
    let (_o, x, y, z) = frame_axes(frame(&vp, g).as_ref());
    vp.run(ScaleGroupCommand {
        group: g,
        center: v(0., 0.),
        factor: ScaleFactor::PerAxis([3.0, 1.0, 1.0]),
        axes: Some((x, y, z)),
    });
    let extent = |vp: &Viewport, axis: DVec3| {
        let grp = vp.scene.doc.find_group(g).unwrap();
        let m = vp.scene.doc.mesh(grp.mesh);
        let vals: Vec<f64> = m
            .verts
            .iter()
            .map(|(_, vx)| vx.position.dot(axis))
            .collect();
        vals.iter().cloned().fold(f64::MIN, f64::max)
            - vals.iter().cloned().fold(f64::MAX, f64::min)
    };
    assert!((extent(&vp, x) - 6.0).abs() < 1e-4);
    assert!((extent(&vp, y) - 1.0).abs() < 1e-4);
    assert!(vp.undo());
    assert!((extent(&vp, x) - 2.0).abs() < 1e-4);
}

// ---- Change Axes -------------------------------------------------------------

fn world_pts(vp: &Viewport, g: GroupId) -> Vec<[i64; 3]> {
    let wm = world_mesh(&vp.scene.doc.meshes, vp.scene.doc.find_group(g).unwrap());
    let mut pts: Vec<[i64; 3]> = wm
        .verts
        .iter()
        .map(|(_, vx)| {
            let p = vx.position;
            [
                (p.x * 1e5).round() as i64,
                (p.y * 1e5).round() as i64,
                (p.z * 1e5).round() as i64,
            ]
        })
        .collect();
    pts.sort();
    pts
}

fn frame_at(o: DVec3, deg: f64) -> DMat4 {
    DMat4::from_translation(o) * rot_z(deg)
}

#[test]
fn test_change_axes_on_a_group_moves_nothing() {
    let mut vp = stub();
    let g = slab(&mut vp);
    let before = world_pts(&vp, g);
    let new = frame_at(v(2., 1.), 45.0);
    vp.run(ChangeAxesCommand {
        group: g,
        frame: new,
    });
    let f = frame(&vp, g).unwrap();
    assert!(f.abs_diff_eq(new, 1e-9) && world_pts(&vp, g) == before);
    assert!(vp.undo());
    assert!(frame(&vp, g).is_none());
}

#[test]
fn test_change_axes_on_a_component_keeps_every_copy_in_place() {
    let mut vp = stub();
    let proto = add_mesh(&mut vp.scene.doc.meshes, slab_mesh());
    let mut a = Group::new(proto, Some("a"));
    a.xform = Some(frame_at(v(0., 0.), 0.0));
    let mut b = Group::new(proto, Some("b"));
    b.xform = Some(frame_at(v(10., 0.), 90.0));
    let (ia, ib) = (a.id, b.id);
    vp.scene.doc.groups.push(a);
    vp.scene.doc.groups.push(b);
    let (before_a, before_b) = (world_pts(&vp, ia), world_pts(&vp, ib));
    let new = frame_at(v(1., 0.5), 30.0);
    vp.run(ChangeAxesCommand {
        group: ia,
        frame: new,
    });
    assert!(world_pts(&vp, ia) == before_a && world_pts(&vp, ib) == before_b);
    let (ma, mb) = (
        vp.scene.doc.find_group(ia).unwrap().mesh,
        vp.scene.doc.find_group(ib).unwrap().mesh,
    );
    assert!(ma == mb && ma == proto); // still one definition
    assert!(close(red(&vp, ia), frame_axes(Some(&new)).1));
    // the copy's axes turned with the definition (30° on top of its 90°)
    assert!(close(
        red(&vp, ib),
        frame_axes(Some(&frame_at(v(0., 0.), 120.0))).1
    ));
    assert!(vp.undo());
    assert!(world_pts(&vp, ia) == before_a && world_pts(&vp, ib) == before_b);
    assert!(close(red(&vp, ia), v(1., 0.)));
}

#[test]
fn test_the_tool_takes_origin_red_and_green() {
    let mut vp = stub();
    let g = slab(&mut vp);
    let mut tool = ChangeAxesTool::for_target(g);
    tool.on_activate(&mut vp);
    for p in [v(2., 0.), v(2., 5.), v(0., 3.)] {
        // origin, red, green
        tool.on_click(&mut vp, &ToolContext::at(p));
    }
    let (o, x, y, z) = frame_axes(frame(&vp, g).as_ref());
    assert!(close(o, v(2., 0.)) && close(x, v(0., 1.)));
    assert!(close(y, v(-1., 0.)) && close(z, v3(0., 0., 1.)));
}

#[test]
fn test_a_pasted_rotated_group_keeps_its_axes() {
    // Copy turns a classic group into an instance snapshot and Paste
    // copies that — the axes must come along.
    let mut vp = stub();
    let g = slab(&mut vp);
    rotate(&mut vp, g, 30.0);
    let mut src = vp.scene.doc.find_group(g).unwrap().clone();
    let mut tpl = copy_group(&mut vp.scene.doc.meshes, &mut src, None, false); // what Copy snapshots
    if tpl.xform.is_none() {
        tpl.xform = Some(DMat4::IDENTITY);
    }
    let pasted = copy_group(&mut vp.scene.doc.meshes, &mut tpl, Some(v(5., 0.)), false); // what Paste stamps
    let f = group_frame(&pasted).unwrap();
    let red = f.x_axis.truncate();
    assert!((red.x - 0.866025).abs() < 1e-4 && (red.y - 0.5).abs() < 1e-4);
    assert!(close(f.w_axis.truncate(), v(5., 0.)));
}
