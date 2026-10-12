// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Solids tools: the tool tests (the operations' tests live in
//! crates/xtrude-model).

use crate::common;
use crate::common::FakeProbe;

use std::cell::RefCell;
use std::rc::Rc;

use common::{stub, v3};
use xtrude_app::tools::solids::SolidTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::solids::{solid_volume, SolidOp};

fn box_mesh(lo: [f64; 3], hi: [f64; 3], color: [f64; 3], open: bool) -> Mesh {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    let p = v3;
    let faces = [
        [p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0)],
        [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
        [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
        [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
        [p(x1, y1, z0), p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1)],
        [p(x0, y1, z0), p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1)],
    ];
    let mut m = Mesh::new();
    for (i, pts) in faces.iter().enumerate() {
        if open && i == 0 {
            continue;
        }
        let f = m.add_face(pts, &[]);
        m.face_mut(f).attrs.paint.color = Some(color);
    }
    m
}

fn add_box(vp: &mut Viewport, lo: [f64; 3], hi: [f64; 3], name: &str, open: bool) -> GroupId {
    let m = box_mesh(lo, hi, [1.0, 0.0, 0.0], open);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some(name));
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    id
}

fn pair(vp: &mut Viewport) -> (GroupId, GroupId) {
    (
        add_box(vp, [0., 0., 0.], [2., 2., 2.], "A", false),
        add_box(vp, [1., 1., 1.], [3., 3., 3.], "B", false),
    )
}

/// `pick_group` answers the queued groups one per call.
fn picks(vp: &mut Viewport, queue: Vec<GroupId>) {
    let q = Rc::new(RefCell::new(queue));
    FakeProbe::of(vp).pick_group(move |_, _| {
        let mut q = q.borrow_mut();
        (!q.is_empty()).then(|| q.remove(0))
    });
}

fn click(tool: &mut SolidTool, vp: &mut Viewport) {
    tool.on_click(vp, &ToolContext::at(v3(0., 0., 0.)));
}

fn said(vp: &mut Viewport) -> Vec<String> {
    vp.outbox
        .events
        .drain(..)
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

fn only_volume(vp: &Viewport) -> f64 {
    assert_eq!(vp.scene.doc.groups.len(), 1);
    solid_volume(&vp.scene.doc.meshes, &vp.scene.doc.groups[0]).expect("a solid")
}

#[test]
fn test_the_tool_picks_one_then_two_and_the_first_cuts() {
    let mut vp = stub();
    let (a, b) = pair(&mut vp);
    picks(&mut vp, vec![a, b]);
    let mut tool = SolidTool::new(SolidOp::Subtract);
    tool.on_activate(&mut vp);
    click(&mut tool, &mut vp);
    assert!(tool.first == Some(a) && tool.dragging());
    click(&mut tool, &mut vp);
    assert!(tool.first.is_none());
    assert!((only_volume(&vp) - 7.0).abs() < 1e-6 * 7.0);
}

#[test]
fn test_the_tool_refuses_what_is_not_a_solid() {
    let mut vp = stub();
    add_box(&mut vp, [0., 0., 0.], [2., 2., 2.], "A", false);
    let open = add_box(&mut vp, [5., 5., 5.], [6., 6., 6.], "box", true);
    picks(&mut vp, vec![open]);
    let mut tool = SolidTool::new(SolidOp::Union);
    tool.on_activate(&mut vp);
    click(&mut tool, &mut vp);
    assert!(tool.first.is_none() && !said(&mut vp).is_empty());
}

#[test]
fn test_union_runs_at_once_on_a_preselection() {
    let mut vp = stub();
    let (a, b) = pair(&mut vp);
    vp.scene.selection.insert(EntityRef::Group(a));
    vp.scene.selection.insert(EntityRef::Group(b));
    picks(&mut vp, vec![]);
    SolidTool::new(SolidOp::Union).on_activate(&mut vp);
    assert!((only_volume(&vp) - 15.0).abs() < 1e-6 * 15.0);
}
