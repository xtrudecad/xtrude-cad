// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Ctrl+Z inside the Line steps the chain back one vertex; with only the first
//! point placed, the point is let go.

use crate::common;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::{Tool, ToolValue};
use xtrude_app::viewport::Viewport;

fn p(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn draw(t: &mut LineTool, vp: &mut Viewport, pts: &[DVec3]) {
    t.start_point = Some(pts[0]);
    t.chain_first_point = Some(pts[0]);
    t.chain_vertices = vec![pts[0]];
    for &q in &pts[1..] {
        t.hover_point = Some(q);
        let d = q - t.start_point.unwrap();
        assert!(t.on_value(vp, &ToolValue::Triple(d.x, d.y, d.z)));
    }
}

fn edges(vp: &Viewport) -> usize {
    vp.scene.mesh().edge_ids().len()
}

#[test]
fn test_undo_steps_the_chain_back_to_the_previous_vertex() {
    let mut vp = stub();
    let mut t = LineTool::new();
    draw(&mut t, &mut vp, &[p(0., 0.), p(2., 0.), p(2., 3.)]);
    assert_eq!(edges(&vp), 2);
    assert_eq!(t.on_undo(&mut vp), Some(true));
    assert_eq!(edges(&vp), 1); // the segment gone
    assert!((t.start_point.unwrap() - p(2., 0.)).length() < 1e-9); // not (2, 3)
    assert_eq!(t.on_undo(&mut vp), Some(true));
    assert_eq!(edges(&vp), 0);
    assert!((t.start_point.unwrap() - p(0., 0.)).length() < 1e-9);
    assert_eq!(t.on_undo(&mut vp), Some(true)); // the first point…
    assert!(t.start_point.is_none()); // …let go
    assert_eq!(vp.history.undo_len(), 0);
}

#[test]
fn test_idle_line_leaves_undo_to_the_history() {
    let mut vp = stub();
    assert_eq!(LineTool::new().on_undo(&mut vp), None);
}

#[test]
fn test_the_chain_goes_on_from_the_stepped_back_vertex() {
    let mut vp = stub();
    let mut t = LineTool::new();
    draw(&mut t, &mut vp, &[p(0., 0.), p(2., 0.), p(2., 3.)]);
    t.on_undo(&mut vp);
    t.hover_point = Some(p(2., -1.));
    assert!(t.on_value(&mut vp, &ToolValue::Triple(0.0, -1.0, 0.0)));
    let m = vp.scene.mesh();
    let ends: Vec<DVec3> = m
        .edge_ids()
        .into_iter()
        .flat_map(|e| {
            let (a, b) = m.edge_ab(e);
            [a, b]
        })
        .collect();
    assert!(ends.iter().any(|q| (*q - p(2., -1.)).length() < 1e-9)); // left from (2, 0)
    assert!(!ends.iter().any(|q| (*q - p(2., 3.)).length() < 1e-9));
}
