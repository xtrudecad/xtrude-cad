// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The 2-point Arc started on an edge is tangent to it; on the adjacent edge
//! at the same distance from the corner it snaps magenta and a double-click
//! rounds and trims the corner; a typed value there is the radius.

use crate::common;

use common::{sized_viewport, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::arc::{last_fillet_d, ArcTool, BulgeKind, COLOR_FILLET};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::snap::COLOR_TANGENT;
use xtrude_core::EdgeId;
use xtrude_model::edits::build_add_edges;

fn square(size: f64) -> Viewport {
    let mut vp = sized_viewport(1000., 700.);
    let sq = [
        v3(0., 0., 0.),
        v3(size, 0., 0.),
        v3(size, size, 0.),
        v3(0., size, 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    vp.camera.target = v3(size / 2., size / 2., 0.);
    vp.camera.distance = 8.0;
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    vp
}

fn ctx(vp: &Viewport, world: DVec3) -> ToolContext {
    let mut c = ToolContext::at(world);
    c.screen = vp.world_to_pixel(world).expect("on screen").tuple();
    c
}

fn alt(vp: &Viewport, world: DVec3) -> ToolContext {
    ctx(vp, world).with_modifiers(Modifiers::ALT)
}

fn tool(vp: &mut Viewport) -> ArcTool {
    let mut t = ArcTool::new();
    t.on_activate(vp);
    vp.inference.hover_edge = None;
    t
}

fn arc_edges(vp: &Viewport) -> Vec<EdgeId> {
    let m = vp.scene.mesh();
    m.edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).curve.is_some())
        .collect()
}

/// No edge meets at `p` any more.
fn corner_gone(vp: &Viewport, p: DVec3) -> bool {
    let m = vp.scene.mesh();
    match m.vertex_at(p) {
        None => true,
        Some(v) => !m.edge_ids().iter().any(|&e| {
            let ed = m.edge(e);
            ed.v0 == v || ed.v1 == v
        }),
    }
}

fn rgb(t: &ArcTool) -> Option<[f64; 3]> {
    t.wireframe_color().map(|c| [c[0], c[1], c[2]])
}

fn armed(vp: &mut Viewport) -> (ArcTool, DVec3) {
    let mut t = tool(vp);
    let c1 = ctx(vp, v3(1.5, 0., 0.));
    t.on_click(vp, &c1);
    let p2 = v3(2.0, 0.5, 0.);
    let c2 = ctx(vp, p2);
    t.on_hover(vp, &c2);
    t.on_click(vp, &c2);
    (t, p2)
}

#[test]
fn test_start_on_an_edge_previews_the_tangent_arc() {
    let mut vp = square(2.0);
    let mut t = tool(&mut vp);
    let c = ctx(&vp, v3(1.5, 0., 0.));
    t.on_click(&mut vp, &c);
    assert!(t.start_edge.is_some());
    let h = ctx(&vp, v3(1.0, 1.0, 0.));
    t.on_hover(&mut vp, &h);
    let lines = t.rubber_band_lines(&vp);
    assert!(lines.len() > 1);
    assert_eq!(rgb(&t), Some(COLOR_TANGENT));
    assert_eq!(t.value_label(&vp).unwrap().0, "Tangent to edge");
    let (a, b) = lines[0];
    let d = (b - a).normalize();
    assert!(d.y.abs() < 0.15 && d.z.abs() < 1e-6); // leaves P1 along the edge
}

#[test]
fn test_the_adjacent_edge_snaps_at_the_same_distance_from_the_corner() {
    let mut vp = square(2.0);
    let mut t = tool(&mut vp);
    let c = ctx(&vp, v3(1.5, 0., 0.));
    t.on_click(&mut vp, &c);
    let p2 = v3(2.0, 0.5, 0.);
    let px = vp.world_to_pixel(p2).unwrap();
    let near = (px.x + 3., px.y - 2.);
    let world = vp.world_from_pixel(near.0.trunc(), near.1.trunc()).unwrap();
    let mut h = ToolContext::at(world);
    h.screen = near;
    t.on_hover(&mut vp, &h);
    let (corner, snapped, _edge) = t.equidistant.expect("snapped");
    assert!((snapped - p2).length() < 1e-6);
    assert!((corner - v3(2., 0., 0.)).length() < 1e-9);
    assert_eq!(rgb(&t), Some(COLOR_FILLET));
    assert!((t.hover_point.unwrap() - p2).length() < 1e-6);
    let far = ctx(&vp, v3(2.0, 1.5, 0.)); // far along the same edge
    t.on_hover(&mut vp, &far);
    assert!(t.equidistant.is_none());
    assert_ne!(rgb(&t), Some(COLOR_FILLET));
}

#[test]
fn test_double_click_on_the_magenta_point_fillets_and_trims_the_corner() {
    let mut vp = square(2.0);
    let (mut t, p2) = armed(&mut vp);
    assert!(t.fillet.is_some());
    assert!((t.end_point.unwrap() - p2).length() < 1e-6);
    let c = ctx(&vp, p2);
    t.on_double_click(&mut vp, &c);
    assert!(t.start_point.is_none()); // committed and reset
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(corner_gone(&vp, v3(2., 0., 0.)));
    let arc = arc_edges(&vp);
    assert_eq!(arc.len(), t.segments);
    let centre = v3(1.5, 0.5, 0.);
    for e in arc {
        let (a, b) = vp.scene.mesh().edge_ab(e);
        assert!(((a - centre).length() - 0.5).abs() < 1e-6);
        assert!(((b - centre).length() - 0.5).abs() < 1e-6);
    }
    assert!((last_fillet_d().unwrap() - 0.5).abs() < 1e-9);
    assert!(vp.undo()); // one undo brings the square corner back
    assert!(!corner_gone(&vp, v3(2., 0., 0.)));
    assert!(vp.scene.mesh().faces.len() == 1 && vp.scene.mesh().edges.len() == 4);
}

#[test]
fn test_bulge_phase_snaps_to_the_fillet_and_to_the_half_circle() {
    let mut vp = square(2.0);
    let (mut t, p2) = armed(&mut vp);
    let p1 = v3(1.5, 0., 0.);
    let mid = (p1 + p2) * 0.5;
    let half = (p2 - p1).length() / 2.0;
    let h_fillet = 0.5 - (0.25 - half * half).sqrt();
    let toward = (v3(2., 0., 0.) - mid).normalize();
    let at_fillet = ctx(&vp, mid + toward * h_fillet);
    t.on_hover(&mut vp, &at_fillet);
    assert_eq!(t.bulge_kind, Some(BulgeKind::Fillet));
    assert_eq!(rgb(&t), Some(COLOR_FILLET));
    assert!(t.value_label(&vp).unwrap().0.starts_with("Tangent to edge"));
    let h = ctx(&vp, mid - toward * half);
    t.on_hover(&mut vp, &h);
    assert_eq!(t.bulge_kind, Some(BulgeKind::Half));
    assert!(t.value_label(&vp).unwrap().0.starts_with("Half circle"));
    let h = ctx(&vp, mid - toward * (half * 2.5));
    t.on_hover(&mut vp, &h);
    assert!(t.bulge_kind.is_none());
    // a single click at the fillet commits AND trims
    t.on_hover(&mut vp, &at_fillet);
    t.on_click(&mut vp, &at_fillet);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(corner_gone(&vp, v3(2., 0., 0.)));
}

#[test]
fn test_alt_keeps_the_corner_stubs() {
    let mut vp = square(2.0);
    let (mut t, p2) = armed(&mut vp);
    let c = alt(&vp, p2);
    t.on_double_click(&mut vp, &c);
    assert!(t.start_point.is_none());
    assert!(!corner_gone(&vp, v3(2., 0., 0.)));
    assert_eq!(vp.scene.mesh().faces.len(), 2); // the corner sliver stays
}

#[test]
fn test_double_click_near_another_corner_repeats_the_last_distance() {
    let mut vp = square(2.0);
    let (mut t, p2) = armed(&mut vp);
    let c = ctx(&vp, p2);
    t.on_double_click(&mut vp, &c);
    assert!((last_fillet_d().unwrap() - 0.5).abs() < 1e-9);
    let near = ctx(&vp, v3(0.02, 1.98, 0.));
    t.on_click(&mut vp, &near);
    t.on_double_click(&mut vp, &near);
    assert!(t.start_point.is_none());
    assert!(corner_gone(&vp, v3(0., 2., 0.)));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    let centre = v3(0.5, 1.5, 0.);
    let n = arc_edges(&vp)
        .into_iter()
        .filter(|&e| ((vp.scene.mesh().edge_ab(e).0 - centre).length() - 0.5).abs() < 1e-6)
        .count();
    assert_eq!(n, t.segments);
}

#[test]
fn test_a_three_edge_corner_is_not_trimmed() {
    let mut vp = square(2.0);
    let cmd = build_add_edges(
        &vp.scene,
        &[(v3(2., 0., 0.), v3(2., 0., 1.))],
        true,
        vec![],
        None,
    );
    vp.execute_boxed(cmd); // a post on the corner
    let mut t = tool(&mut vp);
    let c1 = ctx(&vp, v3(1.5, 0., 0.));
    t.on_click(&mut vp, &c1);
    let p2 = v3(2.0, 0.5, 0.);
    let c2 = ctx(&vp, p2);
    t.on_hover(&mut vp, &c2);
    assert!(t.equidistant.is_some()); // the inference still works
    t.on_click(&mut vp, &c2);
    t.on_double_click(&mut vp, &c2);
    assert!(!corner_gone(&vp, v3(2., 0., 0.)));
    assert_eq!(vp.scene.mesh().faces.len(), 2);
}

#[test]
fn test_segments_typed_after_the_arc_rebuild_it() {
    let mut vp = square(2.0);
    let (mut t, p2) = armed(&mut vp);
    let c = ctx(&vp, p2);
    t.on_double_click(&mut vp, &c);
    assert_eq!(arc_edges(&vp).len(), 16);
    assert!(t.on_segments_value(&mut vp, 6));
    assert_eq!(arc_edges(&vp).len(), 6);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(corner_gone(&vp, v3(2., 0., 0.)));
    assert_eq!(parse_value_buffer("6s", false), Some(Typed::Segments(6)));
    assert_eq!(parse_value_buffer("2.5r", false), Some(Typed::Radius(2.5)));
}

#[test]
fn test_the_fillet_follows_the_faces_plane() {
    let mut vp = sized_viewport(1000., 700.);
    let sq = [
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 0., 2.),
        v3(0., 0., 2.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    vp.camera.target = v3(1., 0., 1.);
    vp.camera.distance = 8.0;
    vp.camera.pitch = 0.0;
    vp.camera.yaw = (-90f64).to_radians();
    let mut t = tool(&mut vp);
    let plane = Some((DVec3::ZERO, DVec3::Y));
    t.lock.hover_plane = plane;
    let c1 = ctx(&vp, v3(1.5, 0., 0.));
    t.on_click(&mut vp, &c1);
    let p2 = v3(2.0, 0., 0.5);
    t.lock.hover_plane = plane;
    let c2 = ctx(&vp, p2);
    t.on_hover(&mut vp, &c2);
    assert!(t.equidistant.is_some());
    t.on_click(&mut vp, &c2);
    t.lock.hover_plane = plane;
    t.on_double_click(&mut vp, &c2);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(corner_gone(&vp, v3(2., 0., 0.)));
    for e in arc_edges(&vp) {
        let (a, b) = vp.scene.mesh().edge_ab(e);
        assert!(a.y.abs() < 1e-6 && b.y.abs() < 1e-6);
    }
}

#[test]
fn test_a_typed_value_on_the_magenta_arc_is_the_fillet_radius() {
    let mut vp = square(2.0);
    let (mut t, _p2) = armed(&mut vp);
    assert!(t.fillet.is_some());
    let h = ctx(&vp, v3(1.9, 0.4, 0.)); // anywhere
    t.on_hover(&mut vp, &h);
    assert!(t.on_value(&mut vp, &ToolValue::Num(0.8)));
    assert!(t.start_point.is_none()); // committed
    assert!(corner_gone(&vp, v3(2., 0., 0.)));
    let arc = arc_edges(&vp);
    assert_eq!(arc.len(), t.segments);
    let centre = v3(1.2, 0.8, 0.);
    let mut ends = std::collections::BTreeSet::new();
    for e in arc {
        let (a, b) = vp.scene.mesh().edge_ab(e);
        for p in [a, b] {
            assert!(((p - centre).length() - 0.8).abs() < 1e-6);
            ends.insert(((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64));
        }
    }
    assert!(ends.contains(&(1_200_000, 0)) && ends.contains(&(2_000_000, 800_000)));
    assert!((last_fillet_d().unwrap() - 0.8).abs() < 1e-9);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_a_radius_the_edges_cannot_hold_is_refused_and_said() {
    let _m = MetreDocument::new();
    let mut vp = square(2.0);
    let (mut t, _p2) = armed(&mut vp);
    let h = ctx(&vp, v3(1.9, 0.4, 0.));
    t.on_hover(&mut vp, &h);
    vp.take_events();
    assert!(t.on_value(&mut vp, &ToolValue::Num(3.0))); // 3 m on a 2 m edge
    let said: Vec<String> = vp
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect();
    assert!(said.last().is_some_and(|s| s.contains("3.00")));
    assert!(t.fillet.is_some()); // still armed
    assert!(!corner_gone(&vp, v3(2., 0., 0.)));
}

#[test]
fn test_off_the_fillet_a_typed_value_is_still_the_bulge() {
    let mut vp = square(2.0);
    let mut t = tool(&mut vp);
    let a = ctx(&vp, v3(0.5, 1.0, 0.)); // off any edge
    t.on_click(&mut vp, &a);
    let b = ctx(&vp, v3(1.5, 1.0, 0.));
    t.on_click(&mut vp, &b);
    assert!(t.fillet.is_none());
    let h = ctx(&vp, v3(1.0, 1.3, 0.));
    t.on_hover(&mut vp, &h);
    assert!(t.on_value(&mut vp, &ToolValue::Num(0.2)));
    let top = arc_edges(&vp)
        .into_iter()
        .flat_map(|e| {
            let (a, b) = vp.scene.mesh().edge_ab(e);
            [a.y, b.y]
        })
        .fold(f64::MIN, f64::max);
    assert!((top - 1.2).abs() < 1e-6); // bulge 0.2
}
