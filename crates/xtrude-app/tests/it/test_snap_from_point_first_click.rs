// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport tests: the last hovered corner stays
//! encouraged before the first click; arc midpoints, group origins, arc
//! tangents and circle centres reach inside groups and components. (The engine
//! half is in xtrude-core.)

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{sized_viewport, stub};
use glam::{DMat4, DVec3};
use xtrude_app::input::Modifiers;
use xtrude_app::tools::arc::ArcTool;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::line::LineTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{
    compute_snap, SnapContext, SnapEdge, SnapKind, SnapQuery, SnapResult, COLOR_ENDPOINT,
    COLOR_MIDPOINT, COLOR_ON_EDGE, COLOR_TANGENT,
};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// An elevation: x → right, z → up.
fn w2p(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.z * 100.0))
}

fn snap(edges: &[SnapEdge], cand: DVec3) -> SnapResult {
    let mut q = SnapQuery::new(cand, w2p(cand).unwrap_or_default(), &w2p, 9.0);
    q.edge_threshold_px = Some(14.0);
    compute_snap(&q, edges)
}

fn hover(vp: &mut Viewport, p: DVec3) {
    let ScreenPt { x, y } = vp.world_to_pixel(p).expect("on screen");
    vp.process_hover(x, y, Modifiers::NONE);
}

/// Click the active tool at `world` with no snap (`ToolContext(snap=None)`).
fn click(vp: &mut Viewport, world: DVec3) {
    vp.with_tool(|t, vp| t.on_click(vp, &ToolContext::at(world)));
}

fn snap_kind(vp: &Viewport) -> SnapKind {
    vp.inference
        .last_snap
        .as_ref()
        .map_or(SnapKind::None, |s| s.kind.clone())
}

/// 1000×600, looking down.
fn offscreen_vp() -> Viewport {
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp
}

/// A circle's rim between its first two vertices (a vertex under the
/// cursor would rightly be the encouraged point instead).
fn rim(cx: f64, cy: f64, sides: usize) -> DVec3 {
    let (a0, a1) = (0.0_f64, 2.0 * std::f64::consts::PI / sides as f64);
    v(
        cx + (a0.cos() + a1.cos()) / 2.,
        cy + (a0.sin() + a1.sin()) / 2.,
        0.,
    )
}

/// One half-circle arc (0,0)→(2,0) over (1,1), drawn with the
/// real tool into a scratch scene.
fn arc_mesh() -> Mesh {
    let mut st = stub();
    let mut arc = ArcTool::new();
    for w in [v(0., 0., 0.), v(2., 0., 0.), v(1., 1., 0.)] {
        arc.on_click(&mut st, &ToolContext::at(w));
    }
    let mesh = st.scene.mesh().clone();
    assert!(mesh
        .edge_ids()
        .iter()
        .any(|&e| mesh.edge(e).curve.is_some()));
    mesh
}

/// `mesh` as a group translated by (dx, dy).
fn placed(vp: &mut Viewport, mesh: Mesh, dx: f64, dy: f64) {
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, mesh), Some("part"));
    g.xform = Some(DMat4::from_translation(v(dx, dy, 0.)));
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
}

fn square_mesh() -> Mesh {
    let mut mesh = Mesh::new();
    mesh.add_face(
        &[v(0., 0., 0.), v(2., 0., 0.), v(2., 2., 0.), v(0., 2., 0.)],
        &[],
    );
    mesh
}

#[test]
fn test_the_viewport_keeps_the_hovered_corner_before_the_first_click() {
    let mut vp = sized_viewport(1000., 600.);
    vp.scene.mesh_mut().add_face(
        &[v(0., 0., 0.), v(4., 0., 0.), v(4., 0., 3.), v(0., 0., 3.)],
        &[],
    );
    vp.scene.version += 1;
    vp.camera.set_view("front");
    vp.camera.target = v(2., 0., 1.5);
    vp.camera.distance = 12.0;
    vp.set_active_tool(Some(Box::new(RectangleTool::new())));
    hover(&mut vp, v(4., 0., 3.));
    assert!(vp.inference.acquired_point.is_none()); // crossing it is not enough
    assert!(vp.dwell_active());
    vp.encourage_dwelt(); // …pausing on it is (the timer)
    assert!(vp
        .inference
        .acquired_point
        .is_some_and(|p| (p - v(4., 0., 3.)).length() < 1e-6));
    hover(&mut vp, v(2., 0., 1.));
    assert!((vp.inference.acquired_point.unwrap() - v(4., 0., 3.)).length() < 1e-6);
    // still encouraged
}

#[test]
fn test_hovering_a_circles_rim_encourages_its_centre() {
    let mut vp = offscreen_vp();
    vp.camera.target = v(3., 2., 0.);
    vp.camera.distance = 12.0;
    let tool = RadialTool::circle();
    let sides = tool.sides;
    vp.set_active_tool(Some(Box::new(tool)));
    click(&mut vp, v(2., 2., 0.));
    click(&mut vp, v(3., 2., 0.)); // a circle, centre (2,2), r = 1
    assert!(vp.scene.mesh().edges.len() >= 12);
    vp.set_active_tool(Some(Box::new(RadialTool::circle())));
    hover(&mut vp, rim(2., 2., sides)); // hover the rim…
    vp.encourage_dwelt(); // …and pause on it
    let acquired = vp.inference.acquired_point.expect("an encouraged point");
    assert!((acquired - v(2., 2., 0.)).length() < 1e-3);
    hover(&mut vp, v(5., 2.02, 0.)); // level with the centre, along red
    let snap = vp.inference.last_snap.clone().expect("a snap");
    assert_eq!(snap.kind, SnapKind::FromPoint, "{:?}", snap.kind);
    assert!((snap.point.y - 2.0).abs() < 1e-6 && (snap.point.x - 5.0).abs() < 0.05);
    assert!((snap.guide.unwrap().0 - v(2., 2., 0.)).length() < 1e-3);
}

#[test]
fn test_the_viewport_keeps_the_last_two_encouraged_points() {
    let mut vp = common::viewport();
    for pt in [v(1., 0., 0.), v(2., 0., 0.), v(3., 0., 0.), v(3., 0., 0.)] {
        vp.encourage_point(pt);
    }
    let xs: Vec<f64> = vp.inference.encouraged.iter().map(|p| p.x).collect();
    assert_eq!(xs, vec![2.0, 3.0]);
    assert_eq!(vp.inference.acquired_point.unwrap().x, 3.0);
}

#[test]
fn test_guides_read_on_line_and_group_points_keep_their_colours() {
    // (Magenta group points, the classic way, were tried and dropped: a
    // model made of components turned magenta everywhere.)
    let edges = [
        SnapEdge {
            guide: true, // a guide line
            ..SnapEdge::segment(v(0., 0., 0.), v(4., 0., 0.))
        },
        SnapEdge {
            in_group: true, // a group's edge
            ..SnapEdge::segment(v(0., 0., 2.), v(4., 0., 2.))
        },
    ];
    let r = snap(&edges, v(2., 0., 0.02));
    assert!(r.kind == SnapKind::OnLine && r.color == COLOR_ON_EDGE);
    assert_ne!(snap(&edges, v(4.0, 0., 0.01)).kind, SnapKind::Endpoint); // a guide has no ends
    let r = snap(&edges, v(4.01, 0., 2.01));
    assert!(r.kind == SnapKind::Endpoint && r.color == COLOR_ENDPOINT && r.context.is_none());
    let r = snap(&edges, v(2.0, 0., 2.02));
    assert!(r.kind == SnapKind::Midpoint && r.color == COLOR_MIDPOINT);
}

#[test]
fn test_component_origin_and_arc_midpoint_pseudo_edges_snap_by_name() {
    let edges = [
        SnapEdge {
            component_origin: true,
            ..SnapEdge::segment(v(1., 1., 0.), v(1., 1., 0.))
        },
        SnapEdge {
            arc_midpoint: true,
            ..SnapEdge::segment(v(3., 3., 0.), v(3., 3., 0.))
        },
    ];
    assert_eq!(
        snap(&edges, v(1.02, 1.0, 0.)).kind,
        SnapKind::ComponentOrigin
    );
    assert_eq!(snap(&edges, v(3.0, 3.02, 0.)).kind, SnapKind::ArcMidpoint);
}

#[test]
fn test_the_viewport_offers_an_arcs_sweep_midpoint_and_a_groups_origin() {
    let mut vp = offscreen_vp();
    vp.camera.target = v(3., 2., 0.);
    vp.camera.distance = 12.0;
    vp.set_active_tool(Some(Box::new(ArcTool::new())));
    click(&mut vp, v(0., 0., 0.));
    click(&mut vp, v(2., 0., 0.));
    click(&mut vp, v(1., 1., 0.)); // a half circle, r = 1
    let mut mesh = Mesh::new();
    mesh.add_face(
        &[v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)],
        &[],
    );
    let mut g = Group::new(add_mesh(&mut vp.scene.doc.meshes, mesh), Some("box"));
    g.xform = Some(DMat4::from_translation(v(5., 4., 0.)));
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    hover(&mut vp, v(1., 1.02, 0.)); // the arc's apex
    assert_eq!(snap_kind(&vp), SnapKind::ArcMidpoint);
    assert!((vp.inference.last_snap.as_ref().unwrap().point - v(1., 1., 0.)).length() < 0.02);
    hover(&mut vp, v(5.02, 4.0, 0.)); // the group's origin
    assert_eq!(snap_kind(&vp), SnapKind::ComponentOrigin);
}

#[test]
fn test_an_arc_started_at_another_arcs_end_snaps_tangent_to_it() {
    // The classic "Tangent at Vertex" (cyan): an S-curve — the half circle
    // over (0..2) continues below (2..4) with the same radius.
    let mut vp = sized_viewport(1000., 600.);
    vp.camera.set_view("top");
    vp.camera.target = v(2., 0., 0.);
    vp.camera.distance = 12.0;
    vp.set_active_tool(Some(Box::new(ArcTool::new())));
    click(&mut vp, v(0., 0., 0.));
    click(&mut vp, v(2., 0., 0.));
    click(&mut vp, v(1., 1., 0.)); // half circle, centre (1,0), r = 1
    let mut arc = ArcTool::new();
    arc.on_activate(&mut vp);
    arc.on_click(&mut vp, &ToolContext::at(v(2., 0., 0.))); // start at the other arc's end
    arc.on_click(&mut vp, &ToolContext::at(v(4., 0., 0.)));
    let td = arc.tangent_dir.expect("a tangent");
    assert!((td - v(0., -1., 0.)).length() < 1e-6); // leaving downward
    arc.on_hover(&mut vp, &ToolContext::at(v(3., -0.9, 0.))); // near the tangent bulge (-1)
    assert!(arc.snap_bulge.is_some_and(|b| (b + 1.0).abs() < 1e-6));
    let c = arc.wireframe_color().expect("a colour");
    assert_eq!([c[0], c[1], c[2]], COLOR_TANGENT);
    let pts = arc.points(arc.hover_point, None, None);
    let apex = pts
        .iter()
        .copied()
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .expect("points");
    assert!((apex - v(3., -1., 0.)).length() < 1e-3);
    assert!(arc.value_label(&vp).unwrap().0.contains("Tangent"));
    arc.on_hover(&mut vp, &ToolContext::at(v(3., -0.5, 0.))); // too far from tangent: free bulge
    assert!(arc.snap_bulge.is_none() && arc.wireframe_color().is_none());
}

// ---- groups and components: the inference reaches inside them ---------------------

#[test]
fn test_the_hovered_edge_can_be_a_components_and_the_tip_says_so() {
    let mut vp = offscreen_vp();
    vp.camera.target = v(6., 5., 0.);
    vp.camera.distance = 12.0;
    placed(&mut vp, square_mesh(), 5., 4.); // square at (5..7, 4..6)
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    hover(&mut vp, v(6., 4.02, 0.)); // its bottom edge, mid
    let edge = vp.inference.hover_edge.clone().expect("a hovered edge");
    assert!(edge.in_group);
    assert_eq!(edge.context, Some(SnapContext::Component));
    assert!((edge.a - edge.b).length() > 1.9); // the world edge (5,4)-(7,4)
    let snap = vp.inference.last_snap.clone().expect("a snap");
    assert!(snap.kind == SnapKind::Midpoint && snap.context == Some(SnapContext::Component));
}

#[test]
fn test_arc_midpoint_and_tangent_reach_arcs_inside_a_component() {
    let mut vp = offscreen_vp();
    vp.camera.target = v(6., 4., 0.);
    vp.camera.distance = 12.0;
    placed(&mut vp, arc_mesh(), 5., 4.); // arc (5,4)→(7,4) over (6,5)
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    hover(&mut vp, v(6., 5.02, 0.)); // the placed arc's apex
    assert_eq!(snap_kind(&vp), SnapKind::ArcMidpoint);
    assert!((vp.inference.last_snap.as_ref().unwrap().point - v(6., 5., 0.)).length() < 0.02);
    vp.set_active_tool(Some(Box::new(ArcTool::new())));
    click(&mut vp, v(7., 4., 0.)); // the placed arc's end
    click(&mut vp, v(9., 4., 0.));
    let td = vp
        .tool::<ArcTool>()
        .and_then(|a| a.tangent_dir)
        .expect("a tangent");
    assert!((td - v(0., -1., 0.)).length() < 1e-6);
}

#[test]
fn test_the_centre_of_a_components_circle_comes_from_its_rim() {
    let mut vp = offscreen_vp();
    vp.camera.target = v(7., 6., 0.);
    vp.camera.distance = 12.0;
    let mut st = stub();
    let mut circ = RadialTool::circle();
    for w in [v(2., 2., 0.), v(3., 2., 0.)] {
        circ.on_click(&mut st, &ToolContext::at(w));
    }
    placed(&mut vp, st.scene.mesh().clone(), 5., 4.); // circle centre (7,6), r = 1
    vp.set_active_tool(Some(Box::new(RadialTool::circle())));
    hover(&mut vp, rim(7., 6., circ.sides));
    let r = vp.valid_center_ref().expect("a centre");
    assert!((r.center - v(7., 6., 0.)).length() < 1e-3);
}

#[test]
fn test_a_loose_edge_and_a_components_edge_under_the_cursor_together() {
    // With loose edges and a component under the cursor, the hover must not
    // die. The nearer of the two must simply win.
    let mut vp = offscreen_vp();
    vp.camera.target = v(6., 5., 0.);
    vp.camera.distance = 12.0;
    placed(&mut vp, square_mesh(), 5., 4.); // component edge (5,4)-(7,4)
    vp.scene
        .mesh_mut()
        .add_edge(v(5., 4.1, 0.), v(7., 4.1, 0.)) // a loose edge right beside it
        .expect("edge");
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(LineTool::new())));
    hover(&mut vp, v(6., 4.09, 0.)); // nearer the loose edge
    let e = vp.inference.hover_edge.clone().expect("a hovered edge");
    assert!(!e.in_group);
    hover(&mut vp, v(6., 4.0, 0.)); // on the component's edge
    assert!(vp.inference.hover_edge.as_ref().is_some_and(|e| e.in_group));
}
