// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotate, Center Arc and Scale by scripted clicks on stand-in viewports;
//! Select's double / triple click; mixed selections turning, moving and
//! scaling together.

use crate::common;
use crate::common::FakeProbe;

use std::collections::BTreeSet;

use common::{draw_rect, stub, v3};
use glam::{DMat4, DVec3};
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::arc::CenterArcTool;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::scale::ScaleTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{FaceHit, Viewport};
use xtrude_core::Mesh;
use xtrude_model::commands::{MakeGroupCommand, RotateVerticesCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

type Key3 = (i64, i64, i64);

fn k3(p: DVec3) -> Key3 {
    let r = |x: f64| (x * 1000.0).round() as i64;
    (r(p.x), r(p.y), r(p.z))
}

fn kk(x: f64, y: f64, z: f64) -> Key3 {
    k3(v3(x, y, z))
}

fn keys(vp: &Viewport, mesh: MeshId) -> Vec<Key3> {
    let mut out: Vec<Key3> = vp
        .scene
        .doc
        .mesh(mesh)
        .verts
        .iter()
        .map(|(_, v)| k3(v.position))
        .collect();
    out.sort();
    out
}

fn loose_keys(vp: &Viewport) -> Vec<Key3> {
    keys(vp, vp.scene.active_mesh_id())
}

fn ctx(x: f64, y: f64, z: f64) -> ToolContext {
    ToolContext::at(v3(x, y, z))
}

fn click(vp: &mut Viewport, t: &mut dyn Tool, x: f64, y: f64) {
    t.on_click(vp, &ctx(x, y, 0.0));
}

fn hover(vp: &mut Viewport, t: &mut dyn Tool, x: f64, y: f64) {
    t.on_hover(vp, &ctx(x, y, 0.0));
}

fn rect(vp: &mut Viewport, x0: f64, y0: f64, x1: f64, y1: f64) {
    draw_rect(
        vp,
        &[
            v3(x0, y0, 0.),
            v3(x1, y0, 0.),
            v3(x1, y1, 0.),
            v3(x0, y1, 0.),
        ],
    );
}

fn select_all_loose(vp: &mut Viewport) {
    let mid = vp.scene.active_mesh_id();
    let m = vp.scene.mesh();
    let ents: Vec<EntityRef> = m
        .face_ids()
        .into_iter()
        .map(|f| EntityRef::Face(mid, f))
        .chain(m.edge_ids().into_iter().map(|e| EntityRef::Edge(mid, e)))
        .collect();
    vp.scene.selection.extend(ents);
}

fn group_of_loose(vp: &mut Viewport) -> GroupId {
    let m = vp.scene.mesh();
    let cmd = MakeGroupCommand::new(m, &m.face_ids(), &m.edge_ids(), false, None);
    vp.run(cmd);
    vp.scene.doc.groups[0].id
}

fn gmesh(vp: &Viewport, g: GroupId) -> MeshId {
    vp.scene.doc.find_group(g).unwrap().mesh
}

fn messages(vp: &mut Viewport) -> usize {
    vp.take_events()
        .into_iter()
        .filter(|e| matches!(e, xtrude_app::viewport::ViewportEvent::Flash(..)))
        .count()
}

#[test]
fn test_rotate_selection_90_degrees_and_undo() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let before = loose_keys(&vp);
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    click(&mut vp, &mut t, 1., 0.);
    hover(&mut vp, &mut t, 0., 1.);
    click(&mut vp, &mut t, 0., 1.);
    let got = loose_keys(&vp);
    assert!(got.contains(&kk(0., 2., 0.)));
    assert!(got.contains(&kk(-2., 6., 0.)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(vp.undo());
    assert_eq!(loose_keys(&vp), before);
    assert!(vp.redo());
    assert_eq!(loose_keys(&vp), got);
}

#[test]
fn test_rotate_typed_angle_via_value_box() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    click(&mut vp, &mut t, 1., 0.);
    hover(&mut vp, &mut t, 1., 1.);
    assert!(t.on_value(&mut vp, &ToolValue::Num(90.0)));
    assert!(loose_keys(&vp).contains(&kk(0., 2., 0.)));
}

#[test]
fn test_rotate_group_as_unit() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    let g = group_of_loose(&mut vp);
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    click(&mut vp, &mut t, 1., 0.);
    hover(&mut vp, &mut t, 0., 1.);
    click(&mut vp, &mut t, 0., 1.);
    let gk = keys(&vp, gmesh(&vp, g));
    assert!(gk.contains(&kk(0., 2., 0.)) && gk.contains(&kk(-2., 6., 0.)));
    assert_eq!(vp.scene.mesh().edges.len(), 0);
    vp.undo();
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(2., 0., 0.)));
}

#[test]
fn test_rotate_with_nothing_flashes_hint() {
    let mut vp = stub();
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    vp.take_events();
    click(&mut vp, &mut t, 0., 0.);
    assert!(t.p.start_point.is_none());
    assert!(messages(&mut vp) > 0);
}

#[test]
fn test_center_arc_draws_tagged_curve_quarter() {
    let mut vp = stub();
    let mut t = CenterArcTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    click(&mut vp, &mut t, 3., 0.);
    hover(&mut vp, &mut t, 0., 3.);
    click(&mut vp, &mut t, 0., 3.);
    let m = vp.scene.mesh();
    assert_eq!(m.edges.len(), 6);
    let ids: BTreeSet<Option<i64>> = m.edge_ids().into_iter().map(|e| m.edge(e).curve).collect();
    assert!(ids.len() == 1 && !ids.contains(&None));
    let ends: BTreeSet<(i64, i64)> = m
        .verts
        .iter()
        .map(|(_, v)| (k3(v.position).0, k3(v.position).1))
        .collect();
    assert!(ends.contains(&(3000, 0)) && ends.contains(&(0, 3000)));
}

#[test]
fn test_center_arc_typed_angle_and_circle_weld() {
    let mut vp = stub();
    let mut c = RadialTool::circle();
    click(&mut vp, &mut c, 0., 0.);
    click(&mut vp, &mut c, 3., 0.);
    let before = vp.scene.mesh().verts.len();
    let mut t = CenterArcTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    click(&mut vp, &mut t, 3., 0.);
    hover(&mut vp, &mut t, 1., 1.);
    assert!(t.on_value(&mut vp, &ToolValue::Num(90.0)));
    assert_eq!(vp.scene.mesh().verts.len(), before);
}

fn grip_at(t: &ScaleTool, params: (f64, f64, f64)) -> xtrude_app::tools::scale::Grip {
    t.grips
        .iter()
        .find(|g| g.params == params)
        .cloned()
        .expect("grip")
}

#[test]
fn test_scale_selection_doubles_about_anchor() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let before = loose_keys(&vp);
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    let g = grip_at(&t, (1.0, 1.0, 0.5));
    t.grab(&mut vp, g, (0.0, 0.0));
    t.commit(&mut vp, [2.0, 2.0, 1.0]);
    let got = loose_keys(&vp);
    assert!(got.contains(&kk(2., 0., 0.)));
    assert!(got.contains(&kk(10., 4., 0.)));
    assert!(vp.undo());
    assert_eq!(loose_keys(&vp), before);
    assert!(vp.redo());
    assert_eq!(loose_keys(&vp), got);
}

#[test]
fn test_scale_typed_factor_and_mirror() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    let g = grip_at(&t, (1.0, 0.5, 0.5));
    t.grab(&mut vp, g, (0.0, 0.0));
    assert!(t.on_value(&mut vp, &ToolValue::Num(-1.0)));
    let got = loose_keys(&vp);
    assert!(got.contains(&kk(2., 0., 0.)));
    assert!(got.contains(&kk(-2., 2., 0.)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_scale_group_as_unit() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    let g = group_of_loose(&mut vp);
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    let grip = grip_at(&t, (1.0, 1.0, 0.5));
    t.grab(&mut vp, grip, (0.0, 0.0));
    t.commit(&mut vp, [1.5, 1.5, 1.0]);
    let gk = keys(&vp, gmesh(&vp, g));
    assert!(gk.contains(&kk(2., 0., 0.)) && gk.contains(&kk(8., 3., 0.)));
    vp.undo();
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(6., 2., 0.)));
}

#[test]
fn test_select_all_then_rotate_whole_model_is_rigid() {
    let mut vp = stub();
    rect(&mut vp, 0., 0., 4., 4.);
    rect(&mut vp, 6., 0., 9., 2.);
    let mut positions = Vec::new();
    let mut seen = BTreeSet::new();
    {
        let m = vp.scene.mesh();
        for e in m.edge_ids() {
            let (a, b) = m.edge_ab(e);
            for p in [a, b] {
                if seen.insert(k3(p)) {
                    positions.push(p);
                }
            }
        }
        for f in m.face_ids() {
            for p in m.face_positions(f) {
                if seen.insert(k3(p)) {
                    positions.push(p);
                }
            }
        }
    }
    let f0 = vp.scene.mesh().faces.len();
    vp.run(RotateVerticesCommand {
        src: positions,
        center: DVec3::ZERO,
        axis: DVec3::Z,
        degrees: 37.0,
    });
    assert_eq!(vp.scene.mesh().faces.len(), f0);
    let d = vp
        .scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position.length())
        .fold(f64::MAX, f64::min);
    assert!(d < 1e-5);
}

/// A 4 × 4 × 3 cube pushed with the tool.
fn cube(vp: &mut Viewport) {
    rect(vp, 0., 0., 4., 4.);
    let f = vp.scene.mesh().face_ids()[0];
    let mut pp = PushPullTool::new();
    pp.hovered_face = Some((MeshId::LOOSE, f));
    pp.on_click(vp, &ctx(0., 0., 0.));
    pp.set_extrusion(3.0);
    pp.commit(vp);
}

fn pick_face(vp: &mut Viewport, f: xtrude_core::FaceId) {
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
}

fn top_face(vp: &Viewport) -> xtrude_core::FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - 3.0).abs() < 1e-6))
        .unwrap()
}

#[test]
fn test_double_click_face_selects_face_plus_edges() {
    let mut vp = stub();
    cube(&mut vp);
    let top = top_face(&vp);
    pick_face(&mut vp, top);
    let mut t = SelectTool::new();
    t.on_double_click(&mut vp, &ctx(0., 0., 0.));
    let sel = &vp.scene.selection;
    assert!(sel.contains(&EntityRef::Face(MeshId::LOOSE, top)));
    assert_eq!(sel.iter().filter(|e| e.is_edge()).count(), 4);
}

#[test]
fn test_triple_click_selects_whole_connected_solid() {
    let mut vp = stub();
    cube(&mut vp);
    rect(&mut vp, 10., 10., 12., 12.);
    let top = top_face(&vp);
    pick_face(&mut vp, top);
    let mut t = SelectTool::new();
    t.on_triple_click(&mut vp, &ctx(0., 0., 0.));
    let sel = &vp.scene.selection;
    assert_eq!(sel.iter().filter(|e| e.is_face()).count(), 6);
    assert_eq!(sel.iter().filter(|e| e.is_edge()).count(), 12);
    let m = vp.scene.mesh();
    let far = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f)[0].x >= 10.0)
        .unwrap();
    assert!(!sel.contains(&EntityRef::Face(MeshId::LOOSE, far)));
}

fn square_group(
    vp: &mut Viewport,
    pts: [DVec3; 4],
    name: Option<&str>,
    xform: Option<DMat4>,
) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(&pts, &[]);
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut g = Group::new(id, name);
    g.xform = xform;
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    gid
}

fn rotate_quarter(vp: &mut Viewport, t: &mut RotateTool) {
    click(vp, t, 0., 0.);
    click(vp, t, 1., 0.);
    hover(vp, t, 0., 1.);
    click(vp, t, 0., 1.);
}

#[test]
fn test_ctrl_copy_rotates_a_group_copy() {
    let mut vp = stub();
    let g = square_group(
        &mut vp,
        [
            v3(2., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 1., 0.),
            v3(2., 1., 0.),
        ],
        Some("Plate"),
        None,
    );
    vp.scene.selection.insert(EntityRef::Group(g));
    let before = keys(&vp, gmesh(&vp, g));
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    assert!(t.on_key(&mut vp, Key::Control, Modifiers::NONE));
    rotate_quarter(&mut vp, &mut t);
    assert_eq!(vp.scene.doc.groups.len(), 2);
    assert_eq!(keys(&vp, gmesh(&vp, g)), before);
    let copy = vp.scene.doc.groups.iter().find(|x| x.id != g).unwrap().mesh;
    assert!(keys(&vp, copy).contains(&kk(0., 2., 0.)));
    assert!(vp.undo());
    assert_eq!(
        vp.scene.doc.groups.iter().map(|x| x.id).collect::<Vec<_>>(),
        vec![g]
    );
    assert!(!t.copy);
}

#[test]
fn test_ctrl_copy_of_instance_shares_prototype() {
    let mut vp = stub();
    let inst = square_group(
        &mut vp,
        [
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        Some("Peg"),
        Some(DMat4::IDENTITY),
    );
    let proto = gmesh(&vp, inst);
    vp.scene.selection.insert(EntityRef::Group(inst));
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    rotate_quarter(&mut vp, &mut t);
    let copy = vp.scene.doc.groups.iter().find(|x| x.id != inst).unwrap();
    assert_eq!(copy.mesh, proto);
    assert!(copy.xform.is_some());
}

#[test]
fn test_ctrl_copy_duplicates_loose_faces_rotated() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    t.on_key(&mut vp, Key::Control, Modifiers::NONE);
    rotate_quarter(&mut vp, &mut t);
    assert_eq!(vp.scene.mesh().faces.len(), 2);
    let got = loose_keys(&vp);
    assert!(got.contains(&kk(2., 0., 0.)) && got.contains(&kk(0., 2., 0.)));
}

#[test]
fn test_hot_retype_redoes_the_rotation() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    rotate_quarter(&mut vp, &mut t);
    let depth = vp.history.undo_len();
    assert!(t.on_value(&mut vp, &ToolValue::Num(180.0)));
    assert!(loose_keys(&vp).contains(&kk(-2., 0., 0.)));
    assert_eq!(vp.history.undo_len(), depth);
    assert!(t.on_value(&mut vp, &ToolValue::Num(-90.0)));
    assert!(loose_keys(&vp).contains(&kk(0., -2., 0.)));
    click(&mut vp, &mut t, 9., 9.);
    t.on_cancel(&mut vp);
    assert!(!t.on_value(&mut vp, &ToolValue::Num(15.0)));
}

#[test]
fn test_drag_from_centre_sets_custom_axis() {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|v: DVec3| Some((v.x * 100.0, v.y * 100.0)));
    rect(&mut vp, 2., 0., 6., 2.);
    select_all_loose(&mut vp);
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    hover(&mut vp, &mut t, 3., 0.);
    t.on_release(&mut vp);
    assert!(t.p.custom_axis.is_some());
    assert!((t.p.axis().x - 1.0).abs() < 1e-9);
    let mut t2 = RotateTool::new();
    t2.on_activate(&mut vp);
    click(&mut vp, &mut t2, 0., 0.);
    hover(&mut vp, &mut t2, 0.01, 0.);
    t2.on_release(&mut vp);
    assert!(t2.p.custom_axis.is_none());
}

fn unit_group(vp: &mut Viewport, x0: f64) -> GroupId {
    square_group(
        vp,
        [
            v3(x0, 0., 0.),
            v3(x0 + 1., 0., 0.),
            v3(x0 + 1., 1., 0.),
            v3(x0, 1., 0.),
        ],
        None,
        None,
    )
}

#[test]
fn test_rotate_mixed_selection_groups_and_loose() {
    let mut vp = stub();
    rect(&mut vp, 2., 0., 6., 2.);
    let g1 = unit_group(&mut vp, 10.0);
    let g2 = unit_group(&mut vp, 20.0);
    select_all_loose(&mut vp);
    vp.scene.selection.insert(EntityRef::Group(g1));
    vp.scene.selection.insert(EntityRef::Group(g2));
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    rotate_quarter(&mut vp, &mut t);
    assert!(loose_keys(&vp).contains(&kk(0., 2., 0.)));
    assert!(keys(&vp, gmesh(&vp, g1)).contains(&kk(0., 10., 0.)));
    assert!(keys(&vp, gmesh(&vp, g2)).contains(&kk(0., 20., 0.)));
    assert!(vp.undo());
    assert!(loose_keys(&vp).contains(&kk(2., 0., 0.)));
    assert!(keys(&vp, gmesh(&vp, g1)).contains(&kk(10., 0., 0.)));
    assert!(keys(&vp, gmesh(&vp, g2)).contains(&kk(20., 0., 0.)));
}

#[test]
fn test_move_mixed_selection_groups_and_loose() {
    let mut vp = stub();
    rect(&mut vp, 0., 0., 2., 1.);
    let g = unit_group(&mut vp, 5.0);
    select_all_loose(&mut vp);
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = MoveTool::new();
    t.on_activate(&mut vp);
    click(&mut vp, &mut t, 0., 0.);
    hover(&mut vp, &mut t, 0., 3.);
    click(&mut vp, &mut t, 0., 3.);
    assert!(loose_keys(&vp).contains(&kk(0., 3., 0.)));
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(5., 3., 0.)));
    assert!(vp.undo());
    assert!(loose_keys(&vp).contains(&kk(0., 0., 0.)));
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(5., 0., 0.)));
}

#[test]
fn test_scale_mixed_selection_groups_and_loose() {
    let mut vp = stub();
    rect(&mut vp, 1., 0., 2., 1.);
    let g = unit_group(&mut vp, 4.0);
    select_all_loose(&mut vp);
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    let grip = grip_at(&t, (1.0, 0.5, 0.5));
    t.grab(&mut vp, grip, (0.0, 0.0));
    t.commit(&mut vp, [2.0, 1.0, 1.0]);
    assert!(loose_keys(&vp).contains(&kk(3., 0., 0.)));
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(9., 0., 0.)));
    assert!(vp.undo());
    assert!(loose_keys(&vp).contains(&kk(2., 0., 0.)));
    assert!(keys(&vp, gmesh(&vp, g)).contains(&kk(5., 0., 0.)));
}
