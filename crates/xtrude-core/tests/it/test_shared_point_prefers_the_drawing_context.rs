// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A near-tie goes to the loose vertex.

use crate::snap_common;
use snap_common::*;
use xtrude_core::snap::SnapContext;

fn ctx(a: DVec3, b: DVec3, c: Option<SnapContext>) -> SnapEdge {
    SnapEdge {
        context: c,
        ..edge(a, b)
    }
}

fn snap(edges: &[SnapEdge], cand: DVec3) -> SnapResult {
    let mut q = query(cand);
    q.edge_threshold_px = Some(14.0);
    compute_snap(&q, edges)
}

#[test]
fn test_the_loose_vertex_beats_the_components_corner_at_the_same_spot() {
    let comp = ctx(v(2., 0., 0.), v(2., 2., 0.), Some(SnapContext::Component));
    let loose = ctx(v(0., 0., 0.), v(2., 0., 0.), None);
    for edges in [[comp.clone(), loose.clone()], [loose.clone(), comp.clone()]] {
        let r = snap(&edges, v(2.01, 0.01, 0.0));
        assert!(r.kind == SnapKind::Endpoint && r.context.is_none());
    }
}

#[test]
fn test_a_component_corner_a_hair_closer_still_loses_the_tie() {
    let comp = ctx(
        v(2.000001, 0., 0.),
        v(2., 2., 0.),
        Some(SnapContext::Component),
    );
    let loose = ctx(v(0., 0., 0.), v(2., 0., 0.), None);
    assert!(snap(&[comp, loose], v(2.000001, 0.0, 0.0))
        .context
        .is_none());
}

#[test]
fn test_a_clearly_nearer_component_corner_wins_as_before() {
    let comp = ctx(v(2., 0., 0.), v(2., 2., 0.), Some(SnapContext::Component));
    let loose = ctx(v(0., 0., 0.), v(2.05, 0., 0.), None);
    assert_eq!(
        snap(&[loose, comp], v(2.0, 0.0, 0.0)).context,
        Some(SnapContext::Component)
    );
}
