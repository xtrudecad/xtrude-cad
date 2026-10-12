// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A negative length typed into the Line runs the opposite way; zero is
//! refused.

use crate::common;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::{Tool, ToolValue};

#[test]
fn test_negative_length_runs_away_from_the_cursor() {
    let mut vp = stub();
    let mut t = LineTool::new();
    t.start_point = Some(DVec3::ZERO);
    t.hover_point = Some(DVec3::X); // the rubber band along +X
    assert!(t.on_value(&mut vp, &ToolValue::Num(-2.0)));
    let m = vp.scene.mesh();
    let es = m.edge_ids();
    assert_eq!(es.len(), 1);
    let (a, b) = m.edge_ab(es[0]);
    let (a, b) = if a.length() < b.length() {
        (a, b)
    } else {
        (b, a)
    };
    assert!(a.length() < 1e-9);
    assert!((b - DVec3::new(-2., 0., 0.)).length() < 1e-9); // the other way
    assert!((t.start_point.unwrap() - DVec3::new(-2., 0., 0.)).length() < 1e-9);
    // chain goes on
}

#[test]
fn test_zero_length_is_still_refused() {
    let mut vp = stub();
    let mut t = LineTool::new();
    t.start_point = Some(DVec3::ZERO);
    t.hover_point = Some(DVec3::X);
    assert!(!t.on_value(&mut vp, &ToolValue::Num(0.0)));
    assert!(vp.scene.mesh().edge_ids().is_empty());
}
