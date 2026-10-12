// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Auto-split of crossing edges.

use crate::common;
use common::*;
use xtrude_core::geometry::Segment;
use xtrude_core::topology::{
    plan_edge_split, same_position, segment_intersection, EdgeSplitPlan, SPLIT_TOLERANCE,
};
use xtrude_model::commands::AddEdgeCommand;
use xtrude_model::edits::build_add_edge;

fn si(
    a: xtrude_core::DVec3,
    b: xtrude_core::DVec3,
    c: xtrude_core::DVec3,
    d: xtrude_core::DVec3,
) -> Option<xtrude_core::DVec3> {
    segment_intersection(a, b, c, d, SPLIT_TOLERANCE)
}

#[test]
fn test_intersection_x_crossing() {
    let p = si(V(0., 0.), V(2., 0.), V(1., -1.), V(1., 1.)).unwrap();
    assert!(same_position(p, V(1., 0.)));
}

#[test]
fn test_intersection_t_junction() {
    let p = si(V(0., 0.), V(2., 0.), V(1., 0.), V(1., 1.)).unwrap();
    assert!(same_position(p, V(1., 0.)));
}

#[test]
fn test_intersection_shared_endpoint() {
    let p = si(V(0., 0.), V(1., 0.), V(1., 0.), V(1., 1.)).unwrap();
    assert!(same_position(p, V(1., 0.)));
}

#[test]
fn test_intersection_no_touch() {
    assert!(si(V(0., 0.), V(1., 0.), V(0., 1.), V(1., 1.)).is_none());
}

#[test]
fn test_intersection_collinear_overlap_is_none() {
    assert!(si(V(0., 0.), V(2., 0.), V(1., 0.), V(3., 0.)).is_none());
}

#[test]
fn test_intersection_skew_in_3d_rejected() {
    assert!(si(
        V3(0., 0., 0.),
        V3(2., 0., 0.),
        V3(1., -1., 1.),
        V3(1., 1., 1.)
    )
    .is_none());
}

#[test]
fn test_intersection_outside_segment_rejected() {
    assert!(si(V(0., 0.), V(2., 0.), V(1., 1.), V(1., 2.)).is_none());
}

#[test]
fn test_plan_x_crossing_splits_both() {
    let existing = [Segment::new(V(0., 0.), V(2., 0.))];
    let EdgeSplitPlan {
        new_segments: new_segs,
        edge_cuts: cuts,
    } = plan_edge_split(&existing, V(1., -1.), V(1., 1.));
    assert_eq!(cuts.len(), 1);
    assert!(same_position(cuts[0].1, V(1., 0.)));
    assert_eq!(new_segs.len(), 2);
    assert!(same_position(new_segs[0].1, V(1., 0.)));
}

#[test]
fn test_plan_t_junction_splits_existing_only() {
    let existing = [Segment::new(V(0., 0.), V(2., 0.))];
    let EdgeSplitPlan {
        new_segments: new_segs,
        edge_cuts: cuts,
    } = plan_edge_split(&existing, V(1., 0.), V(1., 1.));
    assert_eq!(cuts[0].0, 0);
    assert_eq!(new_segs.len(), 1);
}

#[test]
fn test_plan_no_crossing() {
    let existing = [Segment::new(V(0., 0.), V(2., 0.))];
    let EdgeSplitPlan {
        new_segments: new_segs,
        edge_cuts: cuts,
    } = plan_edge_split(&existing, V(0., 1.), V(2., 1.));
    assert!(cuts.is_empty());
    assert_eq!(new_segs, vec![(V(0., 1.), V(2., 1.))]);
}

#[test]
fn test_plan_multiple_crossings_ordered() {
    let existing = [
        Segment::new(V(1., -1.), V(1., 1.)),
        Segment::new(V(2., -1.), V(2., 1.)),
    ];
    let EdgeSplitPlan {
        new_segments: new_segs,
        edge_cuts: cuts,
    } = plan_edge_split(&existing, V(0., 0.), V(3., 0.));
    assert_eq!(cuts.len(), 2);
    assert_eq!(new_segs.len(), 3);
    assert!(same_position(new_segs[0].1, V(1., 0.)));
    assert!(same_position(new_segs[1].1, V(2., 0.)));
}

fn scene_with_edge(
    a: xtrude_core::DVec3,
    b: xtrude_core::DVec3,
) -> (xtrude_model::Scene, xtrude_model::History) {
    let (mut s, mut h) = scene_history();
    h.run(&mut s, AddEdgeCommand::new(a, b));
    (s, h)
}

#[test]
fn test_x_crossing_end_to_end() {
    let (mut s, mut h) = scene_with_edge(V(0., 0.), V(2., 0.));
    let c = build_add_edge(&s, V(1., -1.), V(1., 1.), true, None);
    exec(&mut s, &mut h, c);
    assert_eq!(nedges(&s), 4);
    assert!(has_vertex(&s, V(1., 0.)));
    assert_eq!(
        edge_keyset(&s),
        keyset_from(&[
            (V(0., 0.), V(1., 0.)),
            (V(1., 0.), V(2., 0.)),
            (V(1., -1.), V(1., 0.)),
            (V(1., 0.), V(1., 1.))
        ])
    );
}

#[test]
fn test_t_junction_end_to_end() {
    let (mut s, mut h) = scene_with_edge(V(0., 0.), V(2., 0.));
    let c = build_add_edge(&s, V(1., 0.), V(1., 1.), true, None);
    exec(&mut s, &mut h, c);
    assert_eq!(nedges(&s), 3);
    assert!(has_vertex(&s, V(1., 0.)));
}

#[test]
fn test_crossing_undo_restores_original() {
    let (mut s, mut h) = scene_with_edge(V(0., 0.), V(2., 0.));
    let original = s.mesh().edge_ids();
    let c = build_add_edge(&s, V(1., -1.), V(1., 1.), true, None);
    exec(&mut s, &mut h, c);
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().edge_ids(), original);
}

#[test]
fn test_crossing_redo() {
    let (mut s, mut h) = scene_with_edge(V(0., 0.), V(2., 0.));
    let c = build_add_edge(&s, V(1., -1.), V(1., 1.), true, None);
    exec(&mut s, &mut h, c);
    h.undo(&mut s);
    assert!(h.redo(&mut s));
    assert_eq!(nedges(&s), 4);
}

#[test]
fn test_no_crossing_is_single_edge_no_split() {
    let (mut s, mut h) = scene_with_edge(V(0., 0.), V(2., 0.));
    let c = build_add_edge(&s, V(0., 1.), V(2., 1.), true, None);
    exec(&mut s, &mut h, c);
    assert_eq!(nedges(&s), 2);
}

#[test]
fn test_closing_loop_still_auto_faces() {
    let (mut s, mut h) = scene_history();
    let chain = [V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.), V(0., 0.)];
    for i in 0..4 {
        let c = build_add_edge(&s, chain[i], chain[i + 1], true, None);
        exec(&mut s, &mut h, c);
    }
    assert_eq!(nfaces(&s), 1);
    assert_eq!(nedges(&s), 4);
}
