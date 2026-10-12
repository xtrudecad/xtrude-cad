// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Model dimensions attached to the geometry they measure — they follow a
//! scale and re-measure, Move slides the line while the ends keep their
//! vertices. The .igz tests live in crates/xtrude-io.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::{DMat4, DVec3};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::scale::ScaleTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{
    AddDimensionCommand, DimMoveMode, MakeGroupCommand, MoveDimensionsCommand, MoveVerticesCommand,
    ScaleFactor, ScaleVerticesCommand,
};
use xtrude_model::dimension::Dimension;
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, world_mesh, Group};
use xtrude_model::ids::{MeshId, ObjId};

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn square(vp: &mut Viewport, size: f64) -> FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(size, 0.), v(size, size), v(0., size)], &[])
}

fn add_dim(vp: &mut Viewport, a: DVec3, b: DVec3, off: DVec3) -> ObjId {
    let d = Dimension::new(a, b, off);
    let id = d.id;
    vp.execute(&mut AddDimensionCommand::new(d));
    id
}

fn d_a(vp: &Viewport, id: ObjId) -> DVec3 {
    vp.scene.doc.dimension(id).unwrap().a(&vp.scene.doc_view())
}

fn d_b(vp: &Viewport, id: ObjId) -> DVec3 {
    vp.scene.doc.dimension(id).unwrap().b(&vp.scene.doc_view())
}

fn value(vp: &Viewport, id: ObjId) -> f64 {
    vp.scene
        .doc
        .dimension(id)
        .unwrap()
        .value(&vp.scene.doc_view())
}

fn dim(vp: &Viewport, id: ObjId) -> &Dimension {
    vp.scene.doc.dimension(id).unwrap()
}

fn ctx(x: f64, y: f64) -> ToolContext {
    ToolContext::at(v(x, y))
}

/// The tests' stand-in picks nothing.
fn quiet(vp: &mut Viewport) {
    FakeProbe::of(vp).pick_edge(|_, _| None);
    FakeProbe::of(vp).pick_group(|_, _| None);
    FakeProbe::of(vp).pick_dimension(|_, _| None);
}

// ---- 1. the dimension follows the geometry -------------------------------

#[test]
fn test_a_dimension_on_two_vertices_follows_a_scale_and_re_measures() {
    let mut vp = stub();
    square(&mut vp, 4.0);
    let id = add_dim(&mut vp, v(0., 0.), v(4., 0.), v(0., -1.));
    assert!(dim(&vp, id).anchored() && close(value(&vp, id), 4.0));
    // stretch the square: the right side goes from x=4 to x=6
    vp.execute(&mut MoveVerticesCommand {
        src: vec![v(4., 0.), v(4., 4.)],
        delta: v(2., 0.),
    });
    assert_eq!(d_b(&vp, id), v(6., 0.));
    assert!(close(value(&vp, id), 6.0)); // re-measured
    let (_ap, bp) = dim(&vp, id).line_points(&vp.scene.doc_view());
    assert_eq!(bp, v(6., -1.)); // the line came along
    assert!(vp.undo());
    assert!(close(value(&vp, id), 4.0)); // and back
}

#[test]
fn test_the_scale_tool_command_takes_the_dimension_along() {
    let mut vp = stub();
    square(&mut vp, 4.0);
    let id = add_dim(&mut vp, v(0., 0.), v(4., 0.), v(0., -1.));
    vp.execute(&mut ScaleVerticesCommand {
        src: vec![v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)],
        center: v(0., 0.),
        factor: ScaleFactor::Uniform(1.5),
        axes: None,
    });
    assert!(close(value(&vp, id), 6.0));
    assert_eq!(d_b(&vp, id), v(6., 0.));
    assert!(vp.undo());
    assert!(close(value(&vp, id), 4.0));
}

#[test]
fn test_an_endpoint_off_any_vertex_stays_static() {
    let mut vp = stub();
    square(&mut vp, 4.0);
    let id = add_dim(&mut vp, v(0., 0.), v(2., 0.), v(0., -1.)); // b on a midpoint
    let d = dim(&vp, id);
    assert!(d.anchor_a.is_some() && d.anchor_b.is_none() && !d.anchored());
    vp.execute(&mut MoveVerticesCommand {
        src: vec![v(4., 0.), v(4., 4.)],
        delta: v(2., 0.),
    });
    assert!(d_b(&vp, id) == v(2., 0.) && close(value(&vp, id), 2.0));
}

fn grouped_rect(vp: &mut Viewport) {
    let sq = [v(2., 0.), v(6., 0.), v(6., 2.), v(2., 2.)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
}

fn make_group(vp: &mut Viewport) {
    let (f, e) = (vp.scene.mesh().face_ids(), vp.scene.mesh().edge_ids());
    let mk = MakeGroupCommand::new(vp.scene.mesh(), &f, &e, false, None);
    vp.run(mk);
}

fn scale_group_by_grip(vp: &mut Viewport, g: xtrude_model::ids::GroupId) {
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = ScaleTool::new();
    t.on_activate(vp);
    let grip = t
        .grips
        .iter()
        .find(|gr| gr.params == (1.0, 1.0, 0.5))
        .cloned()
        .expect("the grip");
    t.grab(vp, grip, (0.0, 0.0)); // anchor = corner (2, 0)
    t.commit(vp, [1.5, 1.5, 1.0]);
}

#[test]
fn test_a_dimension_on_a_group_made_from_geometry_follows_the_scale_tool() {
    let mut vp = stub();
    quiet(&mut vp);
    grouped_rect(&mut vp);
    make_group(&mut vp);
    let g = vp.scene.doc.groups[0].clone();
    let id = add_dim(&mut vp, v(2., 0.), v(6., 0.), v(0., -1.));
    let d = dim(&vp, id);
    assert!(d.anchored());
    let a = d.anchor_a.as_ref().unwrap();
    assert!(a.mesh == g.mesh && a.chain == vec![g.id]);
    scale_group_by_grip(&mut vp, g.id);
    assert!(close(value(&vp, id), 6.0)); // 4 m × 1.5
    assert!((d_b(&vp, id) - v(8., 0.)).length() < 1e-9);
    assert!(vp.undo());
    assert!(close(value(&vp, id), 4.0));
}

#[test]
fn test_a_dimension_grouped_with_its_geometry_follows_the_group() {
    let mut vp = stub();
    quiet(&mut vp);
    grouped_rect(&mut vp);
    let id = add_dim(&mut vp, v(2., 0.), v(6., 0.), v(0., -1.)); // loose vertices
    assert!(dim(&vp, id).anchored() && dim(&vp, id).anchor_a.as_ref().unwrap().chain.is_empty());
    make_group(&mut vp);
    let g = vp.scene.doc.groups[0].id;
    assert_eq!(d_a(&vp, id), v(2., 0.)); // re-bound on the read
                                         // (the read is pure; a bind pass refreshes the stored anchor)
    let doc = vp.scene.doc_view();
    let mut d = dim(&vp, id).clone();
    d.bind(&doc);
    assert_eq!(d.anchor_a.as_ref().map(|a| a.chain.clone()), Some(vec![g]));
    scale_group_by_grip(&mut vp, g);
    assert!(close(value(&vp, id), 6.0));
    assert!(vp.undo()); // the scale
    assert!(close(value(&vp, id), 4.0));
    assert!(vp.undo()); // the group itself
    assert!(vp.scene.doc.groups.is_empty());
    assert!(close(value(&vp, id), 4.0));
    let vx = vp.scene.mesh().vertex_at(v(6., 0.)).unwrap();
    vp.scene.mesh_mut().move_vertex(vx, v3(1., 0., 0.));
    assert!(close(value(&vp, id), 5.0)); // and still following
}

#[test]
fn test_far_out_coordinates_still_bind_through_a_placement() {
    let mut vp = stub();
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(2.37, 0.), v(2.37, 1.41), v(0., 1.41)], &[]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut g = Group::new(mid, Some("far"));
    g.xform = Some(
        DMat4::from_translation(v3(412.345, -387.89, 3.2))
            * DMat4::from_rotation_z(33f64.to_radians()),
    );
    let wm = world_mesh(&vp.scene.doc.meshes, &g);
    vp.scene.doc.groups.push(g);
    let doc = vp.scene.doc_view();
    for (_, vx) in wm.verts.iter() {
        let p = vx.position;
        let mut d = Dimension::new(p, p + v(1., 0.), v(0., -1.));
        d.bind(&doc);
        assert!(d.anchor_a.is_some(), "{p:?}");
    }
}

#[test]
fn test_a_dimension_inside_a_component_follows_its_placement() {
    let mut vp = stub();
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut g = Group::new(mid, Some("box"));
    g.xform = Some(DMat4::from_translation(v3(10., 0., 0.)));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    let mut d = Dimension::new(v(10., 0.), v(12., 0.), v(0., -1.));
    d.bind(&vp.scene.doc_view());
    assert!(d.anchored() && d.anchor_a.as_ref().unwrap().chain == vec![gid]);
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    let gm = vp.scene.doc.find_group_mut(gid).unwrap();
    gm.xform = Some(DMat4::from_translation(v3(0., 5., 0.)) * gm.xform.unwrap()); // the instance moves
    assert!((d_a(&vp, id) - v(10., 5.)).length() < 1e-9);
    assert!((d_b(&vp, id) - v(12., 5.)).length() < 1e-9);
    let gm = vp.scene.doc.find_group_mut(gid).unwrap();
    gm.xform = Some(gm.xform.unwrap() * DMat4::from_scale(DVec3::splat(2.0))); // …and is scaled
    assert!(close(value(&vp, id), 4.0));
}

#[test]
fn test_a_vertex_that_is_gone_freezes_the_endpoint() {
    let mut vp = stub();
    let f = square(&mut vp, 4.0);
    let mut d = Dimension::new(v(0., 0.), v(4., 0.), v(0., -1.));
    d.bind(&vp.scene.doc_view());
    assert!(d.anchored());
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    let m = vp.scene.mesh_mut();
    m.remove_face(f);
    for e in m.edge_ids() {
        m.remove_edge(e);
    }
    m.prune_orphan_vertices(); // what an erase does
    assert!(d_a(&vp, id) == v(0., 0.) && d_b(&vp, id) == v(4., 0.)); // last known spot
    let mut d = dim(&vp, id).clone();
    d.bind(&vp.scene.doc_view());
    assert!(!d.anchored()); // and let go
}

// ---- 2. Move slides the line, the extension lines keep their vertices --

#[test]
fn test_move_on_an_anchored_dimension_slides_its_line_only() {
    let mut vp = stub();
    quiet(&mut vp);
    square(&mut vp, 4.0);
    let id = add_dim(&mut vp, v(0., 0.), v(4., 0.), v(0., -1.));
    vp.scene.selection.insert(EntityRef::Dimension(id));
    let mut tool = MoveTool::new();
    tool.on_click(&mut vp, &ctx(2., -1.)); // grab the line
    tool.on_hover(&mut vp, &ctx(3., -3.)); // drag: +1 along, -2 away
    assert_eq!(dim(&vp, id).offset, v(0., -3.)); // only the ⟂ part
    assert!(d_a(&vp, id) == v(0., 0.) && d_b(&vp, id) == v(4., 0.)); // ends hold
    tool.on_click(&mut vp, &ctx(3., -3.));
    assert_eq!(dim(&vp, id).offset, v(0., -3.));
    assert!(vp.scene.mesh().vertex_at(v(0., 0.)).is_some()); // geometry untouched
    assert!(vp.undo());
    assert_eq!(dim(&vp, id).offset, v(0., -1.));
}

#[test]
fn test_moving_the_geometry_with_the_dimension_selected_moves_it_once() {
    let mut vp = stub();
    quiet(&mut vp);
    let f = square(&mut vp, 4.0);
    let id = add_dim(&mut vp, v(0., 0.), v(4., 0.), v(0., -1.));
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    vp.scene.selection.insert(EntityRef::Dimension(id));
    let mut tool = MoveTool::new();
    tool.on_click(&mut vp, &ctx(0., 0.));
    tool.on_click(&mut vp, &ctx(0., 10.)); // everything up 10
    assert!(d_a(&vp, id) == v(0., 10.) && d_b(&vp, id) == v(4., 10.)); // rode with its vertices
    assert_eq!(dim(&vp, id).offset, v(0., -1.)); // the line not shifted twice
}

#[test]
fn test_a_free_dimension_moves_rigidly() {
    let mut vp = stub();
    quiet(&mut vp);
    let id = add_dim(&mut vp, v(1., 1.), v(3., 1.), v(0., -1.)); // on nothing
    assert!(!dim(&vp, id).anchored());
    vp.scene.selection.insert(EntityRef::Dimension(id));
    let mut tool = MoveTool::new();
    tool.on_click(&mut vp, &ctx(2., 0.));
    tool.on_click(&mut vp, &ctx(2., 5.));
    assert!(d_a(&vp, id) == v(1., 6.) && d_b(&vp, id) == v(3., 6.));
    assert_eq!(dim(&vp, id).offset, v(0., -1.));
    assert!(vp.undo());
    assert_eq!(d_a(&vp, id), v(1., 1.));
}

#[test]
fn test_the_command_alone_undoes_cleanly() {
    let mut vp = stub();
    square(&mut vp, 4.0);
    let mut d = Dimension::new(v(0., 0.), v(4., 0.), v(0., -1.));
    d.bind(&vp.scene.doc_view());
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    vp.execute(&mut MoveDimensionsCommand {
        modes: vec![(id, DimMoveMode::Line)],
        delta: v(0., -2.),
    });
    assert_eq!(dim(&vp, id).offset, v(0., -3.));
    assert!(vp.undo());
    assert_eq!(dim(&vp, id).offset, v(0., -1.));
}
