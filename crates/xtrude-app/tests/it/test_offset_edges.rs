// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Offset on edges — a click on an edge takes its run; edges
//! selected first are the run.

use crate::common;
use crate::common::FakeProbe;

use std::collections::BTreeSet;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::offset::{run_from_edge, run_from_edges, OffsetTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{PickedEdge, Viewport, ViewportEvent};
use xtrude_core::offset::offset_chain;
use xtrude_core::EdgeId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

type K = (i64, i64, i64);

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn k(p: DVec3) -> K {
    let r = |x: f64| (x * 1000.0).round() as i64;
    (r(p.x), r(p.y), r(p.z))
}

fn kk(x: f64, y: f64) -> K {
    k(v(x, y))
}

fn keys(pts: &[DVec3]) -> BTreeSet<K> {
    pts.iter().map(|p| k(*p)).collect()
}

fn edge_keys(vp: &Viewport) -> BTreeSet<BTreeSet<K>> {
    let m = vp.scene.mesh();
    m.edge_ids()
        .into_iter()
        .map(|e| {
            let (a, b) = m.edge_ab(e);
            [k(a), k(b)].into_iter().collect()
        })
        .collect()
}

fn pair(a: K, b: K) -> BTreeSet<K> {
    [a, b].into_iter().collect()
}

fn add(vp: &mut Viewport, a: DVec3, b: DVec3) -> EdgeId {
    vp.scene.mesh_mut().add_edge(a, b).unwrap()
}

/// The stand-in: no face, the edge we say under the cursor, no ray.
fn hover_edge(vp: &mut Viewport, e: EdgeId) {
    let m = vp.scene.mesh();
    let (a, b) = m.edge_ab(e);
    let edge = PickedEdge {
        a,
        b,
        edge: Some((MeshId::LOOSE, e)),
        in_group: false,
        context: None,
        group: None,
        soft: false,
        curve: None,
    };
    FakeProbe::of(vp).pick_edge(move |_x, _y| Some(edge.clone()));
}

fn said(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

fn ctx() -> ToolContext {
    ToolContext::at(DVec3::ZERO)
}

#[test]
fn test_offset_chain_slides_an_open_l_either_way() {
    let pts = vec![v(0., 0.), v(2., 0.), v(2., 2.)];
    assert_eq!(
        keys(&offset_chain(&pts, DVec3::Z, 0.5).unwrap()),
        [kk(0., 0.5), kk(1.5, 0.5), kk(1.5, 2.)]
            .into_iter()
            .collect()
    );
    assert_eq!(
        keys(&offset_chain(&pts, DVec3::Z, -0.5).unwrap()),
        [kk(0., -0.5), kk(2.5, -0.5), kk(2.5, 2.)]
            .into_iter()
            .collect()
    );
}

#[test]
fn test_run_from_edge_walks_through_corners_and_stops_at_junctions() {
    let mut vp = stub();
    let e1 = add(&mut vp, v(0., 0.), v(2., 0.));
    add(&mut vp, v(2., 0.), v(2., 2.));
    add(&mut vp, v(2., 2.), v(4., 2.));
    add(&mut vp, v(2., 2.), v(2., 4.));
    let (pts, closed) = run_from_edge(vp.scene.mesh(), e1);
    assert!(!closed);
    assert_eq!(
        pts.iter().map(|p| k(*p)).collect::<Vec<_>>(),
        vec![kk(0., 0.), kk(2., 0.), kk(2., 2.)]
    );
}

#[test]
fn test_run_from_edges_orders_a_selection_and_tells_a_loop() {
    let mut vp = stub();
    let es = vec![
        add(&mut vp, v(0., 0.), v(2., 0.)),
        add(&mut vp, v(2., 2.), v(0., 2.)),
        add(&mut vp, v(2., 0.), v(2., 2.)),
        add(&mut vp, v(0., 2.), v(0., 0.)),
    ];
    let (pts, closed) = run_from_edges(vp.scene.mesh(), &es).unwrap();
    assert!(closed && pts.len() == 4);
    assert!(run_from_edges(vp.scene.mesh(), &es[..2]).is_none());
}

#[test]
fn test_clicking_an_edge_offsets_its_run() {
    let mut vp = stub();
    let e1 = add(&mut vp, v(0., 0.), v(2., 0.));
    add(&mut vp, v(2., 0.), v(2., 2.));
    hover_edge(&mut vp, e1);
    let mut t = OffsetTool::new();
    t.on_activate(&mut vp);
    t.on_hover(&mut vp, &ctx());
    t.on_click(&mut vp, &ctx());
    assert!(t.dragging && t.chain && !t.closed);
    assert!(t.on_value(&mut vp, &ToolValue::Num(0.5)));
    let ek = edge_keys(&vp);
    assert!(ek.contains(&pair(kk(0., 0.5), kk(1.5, 0.5))));
    assert!(ek.contains(&pair(kk(1.5, 0.5), kk(1.5, 2.))));
    assert!(vp.scene.mesh().edges.len() == 4 && vp.scene.mesh().faces.is_empty());
    vp.undo();
    assert_eq!(vp.scene.mesh().edges.len(), 2);
}

#[test]
fn test_edges_selected_first_arm_the_tool() {
    let mut vp = stub();
    let es = [
        add(&mut vp, v(0., 0.), v(2., 0.)),
        add(&mut vp, v(2., 0.), v(2., 2.)),
    ];
    vp.scene.select(
        es.iter().map(|&e| EntityRef::Edge(MeshId::LOOSE, e)),
        SelectMode::Replace,
    );
    let mut t = OffsetTool::new();
    t.on_activate(&mut vp);
    assert!(t.armed && said(&mut vp).iter().any(|s| s.contains("2 edges")));
    t.on_click(&mut vp, &ctx());
    assert!(t.dragging);
    t.on_value(&mut vp, &ToolValue::Num(0.25));
    assert_eq!(vp.scene.mesh().edges.len(), 4);
}

#[test]
fn test_a_straight_run_is_refused_with_a_reason() {
    let mut vp = stub();
    let e1 = add(&mut vp, v(0., 0.), v(2., 0.));
    add(&mut vp, v(2., 0.), v(4., 0.));
    hover_edge(&mut vp, e1);
    let mut t = OffsetTool::new();
    t.on_activate(&mut vp);
    t.on_hover(&mut vp, &ctx());
    t.on_click(&mut vp, &ctx());
    assert!(!t.dragging);
    assert!(said(&mut vp).iter().any(|s| s.contains("not in line")));
}

#[test]
fn test_a_closed_loop_of_bare_edges_offsets_as_a_loop() {
    let mut vp = stub();
    let e1 = add(&mut vp, v(0., 0.), v(4., 0.));
    add(&mut vp, v(4., 0.), v(4., 4.));
    add(&mut vp, v(4., 4.), v(0., 4.));
    add(&mut vp, v(0., 4.), v(0., 0.));
    hover_edge(&mut vp, e1);
    let mut t = OffsetTool::new();
    t.on_activate(&mut vp);
    t.on_hover(&mut vp, &ctx());
    t.on_click(&mut vp, &ctx());
    assert!(t.closed);
    t.on_value(&mut vp, &ToolValue::Num(0.5));
    let ek = edge_keys(&vp);
    assert!(ek.contains(&pair(kk(0.5, 0.5), kk(3.5, 0.5))));
    assert!(ek.contains(&pair(kk(0.5, 3.5), kk(0.5, 0.5))));
}
