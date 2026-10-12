// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Move + Ctrl copies, and "3x" / "/3" lay an array; "5x10m"
//! carries the spacing.

use crate::common;
use xtrude_app::value_box::ArrayKind;

use common::{stub, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_app::viewport::Viewport;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn ctx(x: f64, y: f64) -> ToolContext {
    ToolContext::at(v(x, y))
}

fn square(vp: &mut Viewport, x0: f64) -> FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v(x0, 0.), v(x0 + 1., 0.), v(x0 + 1., 1.), v(x0, 1.)], &[])
}

fn select(vp: &mut Viewport, f: FaceId) {
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
}

fn xs(vp: &Viewport) -> Vec<f64> {
    let m = vp.scene.mesh();
    let mut out: Vec<f64> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let x = m
                .face_positions(f)
                .iter()
                .map(|p| p.x)
                .fold(f64::MAX, f64::min);
            (x * 1e6).round() / 1e6
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

fn ctrl(t: &mut MoveTool, vp: &mut Viewport) -> bool {
    t.on_key(vp, Key::Control, Modifiers::NONE)
}

#[test]
fn test_ctrl_leaves_the_original_and_stamps_a_copy() {
    let mut vp = stub();
    let f = square(&mut vp, 0.0);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_hover(&mut vp, &ctx(1., 0.));
    assert_eq!(xs(&vp), vec![1.0]);
    assert!(ctrl(&mut t, &mut vp));
    assert!(t.copy);
    assert_eq!(xs(&vp), vec![0.0]);
    t.on_hover(&mut vp, &ctx(3., 0.));
    assert_eq!(xs(&vp), vec![0.0]);
    assert_eq!(t.rubber_band_lines(&vp).len(), 1 + 4);
    t.on_click(&mut vp, &ctx(3., 0.));
    assert_eq!(xs(&vp), vec![0.0, 3.0]);
    assert!(!t.copy);
    vp.undo();
    assert_eq!(xs(&vp), vec![0.0]);
}

#[test]
fn test_ctrl_again_turns_copy_off_and_the_original_follows() {
    let mut vp = stub();
    let f = square(&mut vp, 0.0);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    ctrl(&mut t, &mut vp);
    t.on_hover(&mut vp, &ctx(2., 0.));
    assert_eq!(xs(&vp), vec![0.0]);
    ctrl(&mut t, &mut vp);
    assert_eq!(xs(&vp), vec![2.0]);
    t.on_click(&mut vp, &ctx(2., 0.));
    assert_eq!(xs(&vp), vec![2.0]);
}

fn copied_at(dx: f64) -> (Viewport, MoveTool) {
    let mut vp = stub();
    let f = square(&mut vp, 0.0);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    ctrl(&mut t, &mut vp);
    t.on_click(&mut vp, &ctx(dx, 0.));
    (vp, t)
}

#[test]
fn test_external_array_after_a_copy() {
    let (mut vp, mut t) = copied_at(2.0);
    assert_eq!(xs(&vp), vec![0.0, 2.0]);
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::External, None));
    assert_eq!(xs(&vp), vec![0.0, 2.0, 4.0, 6.0]);
    assert!(t.on_array_value(&mut vp, 2, ArrayKind::External, None));
    assert_eq!(xs(&vp), vec![0.0, 2.0, 4.0]);
    vp.undo();
    assert_eq!(xs(&vp), vec![0.0]);
}

#[test]
fn test_internal_array_divides_the_distance() {
    let (mut vp, mut t) = copied_at(6.0);
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::Internal, None));
    assert_eq!(xs(&vp), vec![0.0, 2.0, 4.0, 6.0]);
}

#[test]
fn test_the_array_window_closes_at_the_next_click() {
    let (mut vp, mut t) = copied_at(2.0);
    vp.scene.selection.clear();
    t.on_click(&mut vp, &ctx(5., 5.));
    assert!(t.start_point.is_none());
    t.on_click(&mut vp, &ctx(5., 5.));
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::External, None));
    let f0 = vp.scene.mesh().face_ids()[0];
    select(&mut vp, f0);
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_cancel(&mut vp);
    assert!(!t.on_array_value(&mut vp, 3, ArrayKind::External, None));
}

#[test]
fn test_a_plain_move_offers_no_array() {
    let mut vp = stub();
    let f = square(&mut vp, 0.0);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_click(&mut vp, &ctx(2., 0.));
    assert_eq!(xs(&vp), vec![2.0]);
    assert!(!t.on_array_value(&mut vp, 3, ArrayKind::External, None));
}

#[test]
fn test_a_group_copies_as_a_sibling_and_arrays() {
    let mut vp = stub();
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("Spacer"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Group(gid));
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    ctrl(&mut t, &mut vp);
    t.on_click(&mut vp, &ctx(0., 3.));
    assert_eq!(vp.scene.doc.groups.len(), 2);
    let copy = vp.scene.doc.groups.iter().find(|x| x.id != gid).unwrap();
    assert_eq!(copy.name, "Spacer");
    let miny = |mesh: MeshId| {
        let m = vp.scene.doc.mesh(mesh);
        m.face_positions(m.face_ids()[0])
            .iter()
            .map(|p| p.y)
            .fold(f64::MAX, f64::min)
    };
    assert!((miny(copy.mesh) - 3.0).abs() < 1e-6);
    let orig = vp.scene.doc.find_group(gid).unwrap();
    assert!(miny(orig.mesh).abs() < 1e-6);
    assert!(t.on_array_value(&mut vp, 4, ArrayKind::External, None));
    assert_eq!(vp.scene.doc.groups.len(), 5);
    vp.undo();
    assert_eq!(
        vp.scene.doc.groups.iter().map(|g| g.id).collect::<Vec<_>>(),
        vec![gid]
    );
}

#[test]
fn test_the_value_box_understands_the_array_forms() {
    let _m = MetreDocument::new();
    let p = |s: &str| parse_value_buffer(s, false);
    let arr = |n: u32, k: ArrayKind| {
        Some(Typed::Array {
            count: n,
            kind: k,
            step: None,
        })
    };
    assert_eq!(p("3x"), arr(3, ArrayKind::External));
    assert_eq!(p("3X"), arr(3, ArrayKind::External));
    assert_eq!(p("*3"), arr(3, ArrayKind::External));
    assert_eq!(p("3*"), arr(3, ArrayKind::External));
    assert_eq!(p("/3"), arr(3, ArrayKind::Internal));
    assert_eq!(p("3/"), arr(3, ArrayKind::Internal));
    assert_eq!(p("3"), Some(Typed::Value(3.0)));
    assert_ne!(p("3/4\""), arr(3, ArrayKind::Internal));
}

#[test]
fn test_count_and_spacing_in_one_entry_after_a_copy() {
    let (mut vp, mut t) = copied_at(2.0);
    assert!(t.on_array_value(&mut vp, 3, ArrayKind::External, Some(10.0)));
    assert_eq!(xs(&vp), vec![0.0, 10.0, 20.0, 30.0]);
    vp.undo();
    assert_eq!(xs(&vp), vec![0.0]);
}

#[test]
fn test_count_and_spacing_typed_during_the_ctrl_drag() {
    let mut vp = stub();
    let f = square(&mut vp, 0.0);
    select(&mut vp, f);
    let mut t = MoveTool::new();
    t.on_click(&mut vp, &ctx(0., 0.));
    ctrl(&mut t, &mut vp);
    t.on_hover(&mut vp, &ctx(0.7, 0.));
    assert!(t.on_array_value(&mut vp, 4, ArrayKind::External, Some(2.5)));
    assert_eq!(xs(&vp), vec![0.0, 2.5, 5.0, 7.5, 10.0]);
    assert!(t.start_point.is_none());
}

#[test]
fn test_the_value_box_reads_count_times_spacing() {
    let _m = MetreDocument::new();
    let p = |s: &str| parse_value_buffer(s, false);
    assert_eq!(
        p("5x10m"),
        Some(Typed::Array {
            count: 5,
            kind: ArrayKind::External,
            step: Some(10.0)
        })
    );
    assert_eq!(
        p("5x10"),
        Some(Typed::Array {
            count: 5,
            kind: ArrayKind::External,
            step: Some(10.0)
        })
    );
    assert_eq!(
        p("3*250cm"),
        Some(Typed::Array {
            count: 3,
            kind: ArrayKind::External,
            step: Some(2.5)
        })
    );
    assert_eq!(p("5x-2"), None);
}
