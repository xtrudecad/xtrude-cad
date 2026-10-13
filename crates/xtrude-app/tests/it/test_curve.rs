// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A drawn circle/arc's segments share a curve id, so selecting one selects
//! the whole curve; crossings split it into contours; it survives undo/redo,
//! paste and offset. The document test lives in crates/xtrude-io.

use crate::common;

use std::collections::BTreeSet;
use std::f64::consts::PI;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::offset::OffsetTool;
use xtrude_app::tools::paste::PasteTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::triangulate::triangulate;
use xtrude_core::{EdgeId, Mesh};
use xtrude_model::commands::{
    AddFaceCommand, DeleteFaceCommand, EraseSelectionCommand, RebuildPlanarFacesCommand,
    TagCurveCommand,
};
use xtrude_model::edits::{build_add_edge, build_add_edges};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::Command;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn circle_pts(cx: f64, cy: f64, r: f64, n: usize) -> Vec<DVec3> {
    (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64;
            v(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn ring(pts: &[DVec3]) -> Vec<(DVec3, DVec3)> {
    (0..pts.len())
        .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
        .collect()
}

fn draw_circle_at(vp: &mut Viewport, cx: f64, cy: f64) -> Vec<DVec3> {
    let pts = circle_pts(cx, cy, 5.0, 24);
    let extra: Vec<Box<dyn Command>> = vec![
        Box::new(AddFaceCommand::new(pts.clone())),
        Box::new(TagCurveCommand {
            pts: pts.clone(),
            closed: true,
        }),
    ];
    let cmd = build_add_edges(&vp.scene, &ring(&pts), false, extra, None);
    vp.execute_boxed(cmd);
    pts
}

fn curve_ids(vp: &Viewport) -> BTreeSet<Option<i64>> {
    let m = vp.scene.mesh();
    m.edge_ids().into_iter().map(|e| m.edge(e).curve).collect()
}

fn tagged_ids(vp: &Viewport) -> BTreeSet<i64> {
    curve_ids(vp).into_iter().flatten().collect()
}

fn sizes(vp: &Viewport) -> Vec<usize> {
    let m = vp.scene.mesh();
    let mut s: Vec<usize> = tagged_ids(vp)
        .into_iter()
        .map(|c| {
            m.edge_ids()
                .iter()
                .filter(|&&e| m.edge(e).curve == Some(c))
                .count()
        })
        .collect();
    s.sort();
    s
}

fn areas(vp: &Viewport, ground_only: bool) -> Vec<f64> {
    let m = vp.scene.mesh();
    let mut a: Vec<f64> = m
        .face_ids()
        .into_iter()
        .filter(|&f| !ground_only || m.face_positions(f).iter().all(|p| p.z.abs() < 1e-6))
        .map(|f| m.face_area(f))
        .collect();
    a.sort_by(f64::total_cmp);
    a
}

#[test]
fn test_circle_segments_share_one_curve_id() {
    let mut vp = stub();
    draw_circle_at(&mut vp, 0., 0.);
    let ids = curve_ids(&vp);
    assert!(!ids.contains(&None)); // every segment tagged
    assert_eq!(ids.len(), 1); // all share one id
}

#[test]
fn test_curve_edges_selects_whole_circle() {
    let mut vp = stub();
    draw_circle_at(&mut vp, 0., 0.);
    let m = vp.scene.mesh();
    assert_eq!(m.curve_edges(m.edge_ids()[0]).len(), 24); // one segment → all 24
}

#[test]
fn test_curve_id_survives_undo_redo() {
    let mut vp = stub();
    draw_circle_at(&mut vp, 0., 0.);
    let cid = curve_ids(&vp);
    assert!(vp.undo());
    assert!(vp.scene.mesh().edges.is_empty());
    assert!(vp.redo());
    assert_eq!(curve_ids(&vp), cid); // redo keeps the same curve id
}

#[test]
fn test_two_circles_get_distinct_ids() {
    let mut vp = stub();
    draw_circle_at(&mut vp, 0., 0.);
    draw_circle_at(&mut vp, 100., 100.);
    assert_eq!(curve_ids(&vp).len(), 2);
}

#[test]
fn test_tag_curve_robust_to_split_segments() {
    let mut m = Mesh::new();
    let lp = [v(0., 0.), v(10., 0.), v(10., 10.), v(0., 10.)];
    m.add_edge(v(0., 0.), v(5., 0.)).unwrap(); // the first segment split
    m.add_edge(v(5., 0.), v(10., 0.)).unwrap();
    m.add_edge(v(10., 0.), v(10., 10.)).unwrap();
    m.add_edge(v(10., 10.), v(0., 10.)).unwrap();
    m.add_edge(v(0., 10.), v(0., 0.)).unwrap();
    let cid = m.tag_curve(&lp, true).expect("tagged");
    assert!(m.edge_ids().iter().all(|&e| m.edge(e).curve == Some(cid))); // every piece
    assert_eq!(m.curve_edges(m.edge_ids()[0]).len(), 5);
}

/// The reported case: rectangle → corner square → circle crossing it.
fn user_scenario(vp: &mut Viewport) -> f64 {
    let rect = vec![v(0., 0.), v(12., 0.), v(12., 8.), v(0., 8.)];
    let cmd = build_add_edges(
        &vp.scene,
        &ring(&rect),
        true,
        vec![Box::new(AddFaceCommand::new(rect.clone()))],
        None,
    );
    vp.execute_boxed(cmd);
    let sq = vec![v(0., 0.), v(3.6, 0.), v(3.6, 3.6), v(0., 3.6)];
    let cmd = build_add_edges(&vp.scene, &ring(&sq), true, vec![], None);
    vp.execute_boxed(cmd);
    let r = 3.6;
    let pts = circle_pts(3.6, 3.6, r, 24);
    let extra: Vec<Box<dyn Command>> = vec![
        Box::new(TagCurveCommand {
            pts: pts.clone(),
            closed: true,
        }),
        Box::new(RebuildPlanarFacesCommand::new()),
    ];
    let cmd = build_add_edges(&vp.scene, &ring(&pts), false, extra, None);
    vp.execute_boxed(cmd);
    r
}

#[test]
fn test_circle_crossing_square_two_contours_and_sector_face() {
    let mut vp = stub();
    let r = user_scenario(&mut vp);
    assert_eq!(tagged_ids(&vp).len(), 2); // two contours
    assert_eq!(sizes(&vp), vec![6, 18]); // quarter inside + the rest
    let target = PI * r * r / 4.0;
    assert!(areas(&vp, false).iter().any(|a| (a - target).abs() < 0.2));
}

#[test]
fn test_square_drawn_after_circle_splits_three_areas() {
    let mut vp = stub();
    let pts = circle_pts(0., -2., 4., 24);
    let extra: Vec<Box<dyn Command>> = vec![
        Box::new(TagCurveCommand {
            pts: pts.clone(),
            closed: true,
        }),
        Box::new(RebuildPlanarFacesCommand::new()),
    ];
    let cmd = build_add_edges(&vp.scene, &ring(&pts), false, extra, None);
    vp.execute_boxed(cmd);
    let sq = vec![v(-5., 0.), v(5., 0.), v(5., 10.), v(-5., 10.)];
    let cmd = build_add_edges(
        &vp.scene,
        &ring(&sq),
        true,
        vec![Box::new(AddFaceCommand::new(sq.clone()))],
        None,
    );
    vp.execute_boxed(cmd);
    let a = areas(&vp, false);
    assert_eq!(a.len(), 3); // lens + disc rest + square rest
    assert!(a[0] < a[1] && a[1] < a[2]);
    let disc = 0.5 * 24.0 * 16.0 * (2.0 * PI / 24.0).sin();
    assert!((a.iter().sum::<f64>() - (disc + 100.0 - a[0])).abs() < 1e-3);
    assert_eq!(a.iter().filter(|x| **x > 85.0).count(), 1); // no doubled region
    assert_eq!(tagged_ids(&vp).len(), 2);
    assert!(vp.undo()); // the pristine pre-square state
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert_eq!(vp.scene.mesh().edges.len(), 24);
}

fn click(vp: &mut Viewport, t: &mut dyn Tool, x: f64, y: f64) {
    t.on_click(vp, &ToolContext::at(v(x, y)));
}

fn circle_tool(vp: &mut Viewport, cx: f64, cy: f64, r: f64) {
    let mut t = RadialTool::circle();
    t.work_plane = None;
    click(vp, &mut t, cx, cy);
    click(vp, &mut t, cx + r, cy);
}

fn marker(vp: &mut Viewport) -> xtrude_core::FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v3(20., 20., 5.), v3(24., 20., 5.), v3(24., 24., 5.)], &[])
}

#[test]
fn test_two_circles_intersect_next_to_a_solid() {
    let mut vp = stub();
    let mk = marker(&mut vp);
    for cx in [0.0, 5.0] {
        circle_tool(&mut vp, cx, 0., 4.);
    }
    let ground = areas(&vp, true);
    assert_eq!(ground.len(), 3); // lens + two crescents
    let r: Vec<f64> = ground.iter().map(|a| (a * 10.0).round() / 10.0).collect();
    assert!(r[0] < r[1] && r[1] == r[2]);
    assert!(vp.scene.mesh().has_face(mk)); // 3D geometry untouched
    assert_eq!(sizes(&vp), vec![8, 8, 18, 18]);
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 2); // circle 1 disc + marker
    assert_eq!(tagged_ids(&vp).len(), 1);
}

#[test]
fn test_square_over_circle_next_to_a_solid() {
    let mut vp = stub();
    marker(&mut vp);
    circle_tool(&mut vp, 0., -2., 4.);
    let sq = vec![v(-5., 0.), v(5., 0.), v(5., 10.), v(-5., 10.)];
    let cmd = build_add_edges(
        &vp.scene,
        &ring(&sq),
        true,
        vec![Box::new(AddFaceCommand::new(sq.clone()))],
        None,
    );
    vp.execute_boxed(cmd);
    let ground = areas(&vp, true);
    assert_eq!(ground.len(), 3);
    assert_eq!(ground.iter().filter(|a| **a > 85.0).count(), 1); // no doubled square
}

fn rect(vp: &mut Viewport, x0: f64, y0: f64, x1: f64, y1: f64) {
    let pts = vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
    let cmd = build_add_edges(
        &vp.scene,
        &ring(&pts),
        false,
        vec![Box::new(AddFaceCommand::new(pts.clone()))],
        None,
    );
    vp.execute_boxed(cmd);
}

fn rounded(a: &[f64]) -> Vec<f64> {
    a.iter().map(|x| (x * 10.0).round() / 10.0).collect()
}

#[test]
fn test_two_overlapping_rectangles_split_into_three_regions() {
    let mut vp = stub();
    rect(&mut vp, 0., 0., 6., 4.);
    rect(&mut vp, 4., -2., 9., 2.); // overlaps the corner: a 2×2 lens
    assert_eq!(rounded(&areas(&vp, false)), vec![4.0, 16.0, 20.0]);
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 1); // exact restore
    assert!(vp.redo());
    assert_eq!(vp.scene.mesh().faces.len(), 3);
    // no resurrect: delete a face, draw elsewhere, it stays deleted
    let mut vp2 = stub();
    rect(&mut vp2, 0., 0., 6., 4.);
    let f = vp2.scene.mesh().face_ids()[0];
    let lp = vp2.scene.mesh().face_positions(f);
    vp2.run(DeleteFaceCommand { lp });
    rect(&mut vp2, 10., 10., 14., 13.);
    assert_eq!(rounded(&areas(&vp2, false)), vec![12.0]);
}

#[test]
fn test_paste_keeps_curve_identity() {
    let mut vp = stub();
    circle_tool(&mut vp, 0., 0., 3.);
    let m = vp.scene.mesh();
    let arc = m
        .edge_ids()
        .into_iter()
        .find(|&e| m.edge(e).curve.is_some())
        .unwrap();
    let whole: Vec<EdgeId> = m.curve_edges(arc);
    for e in whole {
        vp.scene.selection.insert(EntityRef::Edge(MeshId::LOOSE, e));
    }
    assert!(vp.copy_selection());
    let mut p = PasteTool::new();
    p.on_activate(&mut vp);
    click(&mut vp, &mut p, 10., 0.);
    assert_eq!(tagged_ids(&vp).len(), 2); // original + pasted, own ids
    let m = vp.scene.mesh();
    assert_eq!(
        m.edge_ids()
            .iter()
            .filter(|&&e| m.edge(e).curve.is_none())
            .count(),
        0
    );
    assert_eq!(sizes(&vp), vec![24, 24]);
}

#[test]
fn test_offset_of_circle_tags_ring_and_undoes_cleanly() {
    let mut vp = stub();
    circle_tool(&mut vp, 0., 0., 3.);
    let edges0 = vp.scene.mesh().edges.len();
    let disc = vp.scene.mesh().face_ids()[0];
    let mut o = OffsetTool::new();
    o.base_face = Some((MeshId::LOOSE, disc));
    o.lp = vp.scene.mesh().face_positions(disc);
    o.normal = Some(vp.scene.mesh().face_normal(disc));
    o.distance = 1.0;
    o.commit_now(&mut vp);
    assert_eq!(tagged_ids(&vp).len(), 2); // source circle + offset ring
    assert_eq!(vp.scene.mesh().faces.len(), 2); // ring + inner
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().edges.len(), edges0); // no leaked ring edges
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(vp.redo());
    assert_eq!(vp.scene.mesh().faces.len(), 2);
}

fn erase(vp: &mut Viewport, edges: &[EdgeId]) {
    let cmd = EraseSelectionCommand::from_entities(vp.scene.mesh(), edges, &[]);
    vp.run(cmd);
}

#[test]
fn test_deleting_one_contour_leaves_the_other() {
    let mut vp = stub();
    user_scenario(&mut vp);
    let m = vp.scene.mesh();
    let outer = tagged_ids(&vp)
        .into_iter()
        .max_by_key(|c| {
            m.edge_ids()
                .iter()
                .filter(|&&e| m.edge(e).curve == Some(*c))
                .count()
        })
        .unwrap();
    let one = m
        .edge_ids()
        .into_iter()
        .find(|&e| m.edge(e).curve == Some(outer))
        .unwrap();
    let selected = m.curve_edges(one);
    assert_eq!(selected.len(), 18); // the whole outer contour
    let ends: Vec<(DVec3, DVec3)> = selected.iter().map(|&e| m.edge_ab(e)).collect();
    erase(&mut vp, &selected);
    let m = vp.scene.mesh();
    let left = m
        .edge_ids()
        .iter()
        .filter(|&&e| m.edge(e).curve.is_some())
        .count();
    assert_eq!(left, 6); // the inner arc intact
    for (a, b) in ends {
        let gone = match (m.vertex_at(a), m.vertex_at(b)) {
            (Some(va), Some(vb)) => m.find_edge(va, vb).is_none(),
            _ => true,
        };
        assert!(gone);
    }
}

#[test]
fn test_line_crossing_circle_splits_into_contours() {
    let mut vp = stub();
    draw_circle_at(&mut vp, 0., 0.);
    let cmd = build_add_edges(&vp.scene, &[(v(-10., 0.), v(10., 0.))], true, vec![], None);
    vp.execute_boxed(cmd);
    assert_eq!(tagged_ids(&vp).len(), 2); // split into two arcs
    assert!(sizes(&vp).iter().sum::<usize>() >= 24); // splits may add pieces
}

#[test]
fn test_deleting_big_rectangle_leaves_clean_faces() {
    let mut vp = stub();
    user_scenario(&mut vp);
    let on_rect = |p: DVec3| {
        p.x.abs() < 1e-6
            || (p.x - 12.0).abs() < 1e-6
            || p.y.abs() < 1e-6
            || (p.y - 8.0).abs() < 1e-6
    };
    let m = vp.scene.mesh();
    let big: Vec<EdgeId> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            m.edge(e).curve.is_none() && on_rect(a) && on_rect(b)
        })
        .collect();
    erase(&mut vp, &big);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    for f in m.face_ids() {
        assert!(m.face(f).holes.is_empty()); // no spurious holes
        let tris = triangulate(&m.face_positions(f), &[], Some(m.face_normal(f)));
        assert!(!tris.is_empty()); // triangulates cleanly
    }
    assert_eq!(rounded(&areas(&vp, false)), vec![10.1, 30.2]); // sector + the rest
}

#[test]
fn test_tangent_lines_split_circle_into_arcs() {
    let mut vp = stub();
    let pts = draw_circle_at(&mut vp, 0., 0.);
    let cmd = build_add_edge(&vp.scene, v(9., 2.), pts[0], true, None);
    vp.execute_boxed(cmd);
    let cmd = build_add_edge(&vp.scene, v(2., 9.), pts[6], true, None);
    vp.execute_boxed(cmd);
    assert_eq!(sizes(&vp), vec![6, 18]); // two arcs at the touch points
    let m = vp.scene.mesh();
    let count = |c: i64| {
        m.edge_ids()
            .iter()
            .filter(|&&e| m.edge(e).curve == Some(c))
            .count()
    };
    let short = tagged_ids(&vp)
        .into_iter()
        .min_by_key(|c| count(*c))
        .unwrap();
    let first = m
        .edge_ids()
        .into_iter()
        .find(|&e| m.edge(e).curve == Some(short))
        .unwrap();
    let sel = m.curve_edges(first);
    erase(&mut vp, &sel);
    let m = vp.scene.mesh();
    assert_eq!(
        m.edge_ids()
            .iter()
            .filter(|&&e| m.edge(e).curve.is_some())
            .count(),
        18
    );
    assert!(vp.undo() && vp.undo() && vp.undo());
    assert_eq!(tagged_ids(&vp).len(), 1); // undoing reunites the circle
}

#[test]
fn test_plain_edge_has_no_curve() {
    let mut m = Mesh::new();
    let e = m.add_edge(v(0., 0.), v(1., 0.)).unwrap();
    assert!(m.edge(e).curve.is_none());
    assert_eq!(m.curve_edges(e), vec![e]);
}
