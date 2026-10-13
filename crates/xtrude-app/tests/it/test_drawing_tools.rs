// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Circle, Polygon, Rotated Rectangle and Arc tools — the geometry they
//! commit.

use crate::common;

use common::{draw_rect, faces_with, push, stub, up, v3, TmpDir};
use glam::DVec3;
use xtrude_app::tools::arc::{ArcTool, ThreePointArcTool};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::rotated_rectangle::RotatedRectangleTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::attrs::Paint;
use xtrude_model::commands::{AddFaceCommand, EraseSelectionCommand};
use xtrude_model::edits::build_add_edges;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

fn click(t: &mut dyn Tool, vp: &mut Viewport, p: DVec3) {
    t.on_click(vp, &ctx(p));
}

fn hover(t: &mut dyn Tool, vp: &mut Viewport, p: DVec3) {
    t.on_hover(vp, &ctx(p));
}

fn circle(vp: &mut Viewport, c: DVec3, rim: DVec3) {
    let mut t = RadialTool::circle();
    click(&mut t, vp, c);
    hover(&mut t, vp, rim);
    click(&mut t, vp, rim);
}

fn corners(vp: &Viewport, f: xtrude_core::FaceId) -> Vec<DVec3> {
    vp.scene.mesh().face_positions(f)
}

fn has_corner(vp: &Viewport, f: xtrude_core::FaceId, p: DVec3) -> bool {
    corners(vp, f).iter().any(|q| (*q - p).length() < 1e-4)
}

#[test]
fn test_circle_commits_24gon_at_radius() {
    let mut vp = stub();
    circle(&mut vp, v(0.0, 0.0), v(2.0, 0.0));
    let faces = vp.scene.mesh().face_ids();
    assert_eq!(faces.len(), 1);
    let c = corners(&vp, faces[0]);
    assert_eq!(c.len(), 24);
    assert!(c.iter().all(|p| (p.length() - 2.0).abs() < 1e-4));
}

#[test]
fn test_polygon_commits_hexagon() {
    let mut vp = stub();
    let mut t = RadialTool::polygon();
    click(&mut t, &mut vp, v(1.0, 1.0));
    hover(&mut t, &mut vp, v(3.0, 1.0));
    click(&mut t, &mut vp, v(3.0, 1.0));
    let f = vp.scene.mesh().face_ids()[0];
    let c = corners(&vp, f);
    assert_eq!(c.len(), 6);
    assert!(c.iter().any(|p| (*p - v(1.0, 1.0)).x > 1.99));
}

#[test]
fn test_circle_radius_via_value_box() {
    let mut vp = stub();
    let mut t = RadialTool::circle();
    click(&mut t, &mut vp, v(0.0, 0.0));
    hover(&mut t, &mut vp, v(1.0, 0.0));
    assert!(t.on_value(&mut vp, &ToolValue::Num(5.0)));
    let f = vp.scene.mesh().face_ids()[0];
    assert!(corners(&vp, f)
        .iter()
        .all(|p| (p.length() - 5.0).abs() < 1e-4));
}

#[test]
fn test_circle_tangent_to_edge_vertex_commits() {
    let mut vp = stub();
    let t = v(3.639848232269287, -18.0);
    for (a, b) in [
        (v(0.0, 0.0), v(36.0, 0.0)),
        (v(36.0, 0.0), v(36.0, -18.0)),
        (v(36.0, -18.0), t),
        (t, v(0.0, -18.0)),
        (v(0.0, -18.0), v(0.0, 0.0)),
    ] {
        let cmd = xtrude_model::edits::build_add_edge(&vp.scene, a, b, true, None);
        vp.execute_boxed(cmd);
    }
    let center = v(3.639848232269287, -14.399999618530273);
    let mut tool = RadialTool::circle();
    click(&mut tool, &mut vp, center);
    hover(&mut tool, &mut vp, t);
    click(&mut tool, &mut vp, t);
    assert!(
        vp.history.last_error.is_none(),
        "{:?}",
        vp.history.last_error
    );
    let disc = 0.5 * 24.0 * 3.6 * 3.6 * (2.0 * std::f64::consts::PI / 24.0).sin();
    let m = vp.scene.mesh();
    assert!(m
        .face_ids()
        .into_iter()
        .any(|f| (m.face_area(f) - disc).abs() < 0.01));
    assert!(m.edge_ids().into_iter().all(|e| m.edge_length(e) > 1e-4));
}

#[test]
fn test_rotated_rectangle_axis_aligned_case() {
    let mut vp = stub();
    let mut t = RotatedRectangleTool::new();
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(4.0, 0.0));
    hover(&mut t, &mut vp, v(2.0, 3.0));
    click(&mut t, &mut vp, v(2.0, 3.0));
    let f = vp.scene.mesh().face_ids()[0];
    assert_eq!(corners(&vp, f).len(), 4);
    for c in [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 3.0), v(0.0, 3.0)] {
        assert!(has_corner(&vp, f, c));
    }
}

#[test]
fn test_rotated_rectangle_diagonal_base_is_rotated() {
    let mut vp = stub();
    let mut t = RotatedRectangleTool::new();
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(3.0, 3.0));
    let perp = v(-1.0, 1.0).normalize();
    let cursor = v(3.0, 3.0) + perp;
    hover(&mut t, &mut vp, cursor);
    click(&mut t, &mut vp, cursor);
    let f = vp.scene.mesh().face_ids()[0];
    assert_eq!(corners(&vp, f).len(), 4);
    for c in [v(0.0, 0.0), v(3.0, 3.0), v(3.0, 3.0) + perp, perp] {
        assert!(has_corner(&vp, f, c));
    }
}

#[test]
fn test_arc_points_span_chord_and_bulge() {
    let mut t = ArcTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.end_point = Some(v(4.0, 0.0));
    let pts = t.points(Some(v(2.0, 1.0)), None, None);
    assert_eq!(pts.len(), 17);
    assert!((pts[0] - v(0.0, 0.0)).length() < 1e-6);
    assert!((pts[16] - v(4.0, 0.0)).length() < 1e-6);
    let max_y = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
    assert!((max_y - 1.0).abs() < 1e-4);
}

#[test]
fn test_arc_flat_bulge_is_straight_chord() {
    let mut t = ArcTool::new();
    t.start_point = Some(v(0.0, 0.0));
    t.end_point = Some(v(4.0, 0.0));
    assert_eq!(
        t.points(Some(v(2.0, 0.0)), None, None),
        vec![v(0.0, 0.0), v(4.0, 0.0)]
    );
}

#[test]
fn test_arc_commits_polyline_edges() {
    let mut vp = stub();
    let mut t = ArcTool::new();
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(4.0, 0.0));
    hover(&mut t, &mut vp, v(2.0, 1.0));
    click(&mut t, &mut vp, v(2.0, 1.0));
    let m = vp.scene.mesh();
    assert_eq!(m.edges.len(), 16);
    assert!(m.edge_ids().into_iter().all(|e| !m.edge(e).soft));
}

#[test]
fn test_3point_arc_passes_through_all_three() {
    let mut vp = stub();
    let mut t = ThreePointArcTool::new();
    click(&mut t, &mut vp, v(0.0, 0.0));
    click(&mut t, &mut vp, v(2.0, 2.0));
    hover(&mut t, &mut vp, v(4.0, 0.0));
    let pts = t.points(v(4.0, 0.0));
    assert!((pts[0] - v(0.0, 0.0)).length() < 1e-6);
    assert!((pts[pts.len() - 1] - v(4.0, 0.0)).length() < 1e-6);
    assert!(pts.iter().any(|p| (*p - v(2.0, 2.0)).length() < 1e-4));
}

#[test]
fn test_typed_number_before_centre_sets_sides() {
    let mut vp = stub();
    let mut t = RadialTool::polygon();
    assert!(t.on_value(&mut vp, &ToolValue::Num(8.0)));
    assert_eq!(t.sides, 8);
    click(&mut t, &mut vp, v(0.0, 0.0));
    hover(&mut t, &mut vp, v(2.0, 0.0));
    click(&mut t, &mut vp, v(2.0, 0.0));
    let f = vp.scene.mesh().face_ids()[0];
    assert_eq!(corners(&vp, f).len(), 8);
}

#[test]
fn test_drawn_circle_outline_is_visible() {
    let mut vp = stub();
    circle(&mut vp, v(0.0, 0.0), v(2.0, 0.0));
    let m = vp.scene.mesh();
    assert!(m.edge_ids().into_iter().all(|e| !m.edge(e).soft));
}

fn pushed_cylinder() -> Viewport {
    let mut vp = stub();
    circle(&mut vp, v(0.0, 0.0), v(2.0, 0.0));
    let circ = faces_with(&vp, 24)[0];
    let d = up(&vp, circ, 3.0);
    push(&mut vp, circ, d, false);
    vp
}

#[test]
fn test_pushed_circle_hides_facets_but_keeps_the_circle_line() {
    let vp = pushed_cylinder();
    let m = vp.scene.mesh();
    let soft: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).soft)
        .collect();
    let hard = m.edges.len() - soft.len();
    assert_eq!(soft.len(), 24);
    assert_eq!(hard, 48);
    assert!(soft.iter().all(|&e| {
        let (a, b) = m.edge_ab(e);
        (b - a).normalize().z.abs() > 0.99
    }));

    let mut vp2 = stub();
    let mut p = RadialTool::polygon();
    click(&mut p, &mut vp2, v(0.0, 0.0));
    hover(&mut p, &mut vp2, v(2.0, 0.0));
    click(&mut p, &mut vp2, v(2.0, 0.0));
    let poly = faces_with(&vp2, 6)[0];
    let d = up(&vp2, poly, 3.0);
    push(&mut vp2, poly, d, false);
    let m2 = vp2.scene.mesh();
    assert!(m2.edge_ids().into_iter().all(|e| !m2.edge(e).soft));
}

#[test]
fn test_cylinder_side_is_one_surface() {
    let vp = pushed_cylinder();
    let m = vp.scene.mesh();
    let side = faces_with(&vp, 4)[0];
    assert_eq!(m.surface_of(side).len(), 24);
    let cap = faces_with(&vp, 24)[0];
    assert_eq!(m.surface_of(cap).len(), 1);
}

#[test]
fn test_deleting_cylinder_side_leaves_no_dangling_seams() {
    let mut vp = pushed_cylinder();
    let side = faces_with(&vp, 4)[0];
    let surface = vp.scene.mesh().surface_of(side);
    let cmd = EraseSelectionCommand::from_entities(vp.scene.mesh(), &[], &surface);
    vp.run(cmd);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    assert!(!m
        .edge_ids()
        .into_iter()
        .any(|e| m.edge(e).soft && m.edge(e).faces.is_empty()));
    vp.undo();
    assert_eq!(vp.scene.mesh().faces.len(), 26);
}

#[test]
fn test_box_face_is_its_own_surface() {
    let mut vp = stub();
    draw_rect(
        &mut vp,
        &[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 4.0)],
    );
    let f = vp.scene.mesh().face_ids()[0];
    let d = up(&vp, f, 3.0);
    push(&mut vp, f, d, false);
    let m = vp.scene.mesh();
    for face in m.face_ids() {
        assert_eq!(m.surface_of(face), vec![face]);
    }
}

#[test]
fn test_soft_survives_snapshot_and_document() {
    let mut vp = pushed_cylinder();
    let n_soft = |vp: &Viewport| {
        let m = vp.scene.mesh();
        m.edge_ids().into_iter().filter(|&e| m.edge(e).soft).count()
    };
    assert_eq!(n_soft(&vp), 24);
    vp.undo();
    vp.redo();
    assert_eq!(n_soft(&vp), 24);
    let dir = TmpDir::new();
    let path = dir.path().join("cyl.xtrude");
    xtrude_io::document::save_scene(&vp.scene, &path).unwrap();
    let mut loaded = stub();
    xtrude_io::document::load_into(&mut loaded.scene, &path, None).unwrap();
    assert_eq!(n_soft(&loaded), 24);
}

const PAINT: [f64; 3] = [0.6, 0.5, 0.4];

fn stone() -> Paint {
    Paint {
        color: Some(PAINT),
        mat: Some("Stone".into()),
        opacity: None,
    }
}

fn painted_square(vp: &mut Viewport, size: f64) {
    let ring = [v(0.0, 0.0), v(size, 0.0), v(size, size), v(0.0, size)];
    draw_rect(vp, &ring);
    let f = vp.scene.mesh().face_ids()[0];
    vp.scene.mesh_mut().face_mut(f).attrs.paint = stone();
}

fn add_edges(vp: &mut Viewport, segs: &[(DVec3, DVec3)]) {
    let cmd = build_add_edges(&vp.scene, segs, true, Vec::new(), None);
    vp.execute_boxed(cmd);
}

#[test]
fn test_a_line_across_a_painted_face_keeps_the_paint_on_both_halves() {
    let mut vp = stub();
    painted_square(&mut vp, 4.0);
    add_edges(&mut vp, &[(v(2.0, 0.0), v(2.0, 4.0))]);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    for f in m.face_ids() {
        assert_eq!(m.face(f).attrs.paint, stone());
    }
}

#[test]
fn test_the_split_survives_undo() {
    let mut vp = stub();
    painted_square(&mut vp, 4.0);
    add_edges(&mut vp, &[(v(2.0, 0.0), v(2.0, 4.0))]);
    assert!(vp.undo());
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1);
    assert_eq!(
        m.face(m.face_ids()[0]).attrs.paint.mat.as_deref(),
        Some("Stone")
    );
}

fn box_faces(vp: &mut Viewport, z: f64) {
    let c = [
        v3(0., 0., 0.),
        v3(4., 0., 0.),
        v3(4., 4., 0.),
        v3(0., 4., 0.),
        v3(0., 0., z),
        v3(4., 0., z),
        v3(4., 4., z),
        v3(0., 4., z),
    ];
    for q in [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let pts: Vec<DVec3> = q.iter().map(|&i| c[i]).collect();
        vp.scene.mesh_mut().add_face(&pts, &[]);
    }
}

#[test]
fn test_a_line_across_one_face_of_a_painted_box_keeps_every_face_painted() {
    let mut vp = stub();
    let z = 3.0;
    box_faces(&mut vp, z);
    for f in vp.scene.mesh().face_ids() {
        vp.scene.mesh_mut().face_mut(f).attrs.paint = stone();
    }
    add_edges(&mut vp, &[(v3(2.0, 0.0, z), v3(2.0, 4.0, z))]);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 7);
    let bare = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face(f).attrs.paint.color.is_none())
        .count();
    assert_eq!(bare, 0, "{bare} faces came out bare");
}

#[test]
fn test_a_notch_drawn_down_to_the_bottom_edge_takes_the_side_paint() {
    let mut vp = stub();
    let wall = [
        v3(0., 0., 0.),
        v3(4., 0., 0.),
        v3(4., 0., 3.),
        v3(0., 0., 3.),
    ];
    draw_rect(&mut vp, &wall);
    let f = vp.scene.mesh().face_ids()[0];
    vp.scene.mesh_mut().face_mut(f).attrs.paint = stone();
    let notch = [
        v3(1., 0., 0.),
        v3(1., 0., 2.),
        v3(2.5, 0., 2.),
        v3(2.5, 0., 0.),
    ];
    add_edges(
        &mut vp,
        &[
            (notch[0], notch[1]),
            (notch[1], notch[2]),
            (notch[2], notch[3]),
        ],
    );
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    for f in m.face_ids() {
        assert_eq!(
            m.face(f).attrs.paint,
            stone(),
            "{}-vertex face came out bare",
            m.face(f).outer.len()
        );
    }
}

#[test]
fn test_a_rectangle_drawn_inside_a_painted_face_takes_its_paint_too() {
    let mut vp = stub();
    painted_square(&mut vp, 6.0);
    let inner = [v(2.0, 2.0), v(4.0, 2.0), v(4.0, 4.0), v(2.0, 4.0)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (inner[i], inner[(i + 1) % 4])).collect();
    add_edges(&mut vp, &segs);
    let m = vp.scene.mesh();
    let small = m
        .face_ids()
        .into_iter()
        .min_by(|a, b| m.face_area(*a).total_cmp(&m.face_area(*b)))
        .unwrap();
    assert_eq!(m.face(small).attrs.paint, stone());
}

#[test]
fn test_a_face_drawn_with_its_own_paint_keeps_it() {
    let mut vp = stub();
    painted_square(&mut vp, 6.0);
    let inner = vec![v(2.0, 2.0), v(4.0, 2.0), v(4.0, 4.0), v(2.0, 4.0)];
    let mut attrs = xtrude_core::attrs::FaceAttrs::default();
    attrs.paint.mat = Some("Glass".into());
    let mut cmd = AddFaceCommand::new(inner);
    cmd.attrs = Some(attrs);
    vp.run(cmd);
    let m = vp.scene.mesh();
    let small = m
        .face_ids()
        .into_iter()
        .min_by(|a, b| m.face_area(*a).total_cmp(&m.face_area(*b)))
        .unwrap();
    assert_eq!(m.face(small).attrs.paint.mat.as_deref(), Some("Glass"));
    assert_eq!(m.face(small).attrs.paint.color, Some(PAINT));
}

#[test]
fn test_a_rectangle_drawn_on_a_box_keeps_the_box_closed() {
    let mut vp = stub();
    box_faces(&mut vp, 3.0);
    assert!(xtrude_core::orient::is_closed(vp.scene.mesh()));
    let mut t = RectangleTool::new();
    t.work_plane = Some((v3(0., 0., 0.), v3(0., -1., 0.)));
    for p in [v3(1., 0., 0.), v3(2.5, 0., 2.)] {
        click(&mut t, &mut vp, p);
    }
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 7);
    assert!(
        xtrude_core::orient::is_closed(m),
        "the notch opened the box"
    );
    let lonely = m
        .edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).faces.len() != 2)
        .count();
    assert_eq!(lonely, 0, "{lonely} edges left carrying one face");
}
