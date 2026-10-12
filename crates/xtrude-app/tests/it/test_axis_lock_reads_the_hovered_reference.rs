// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! With an arrow-key lock, the cursor on a REFERENCE far from the locked line
//! lands the snap on that reference's foot.

use crate::common;
use xtrude_core::Axis;

use common::{sized_viewport, v3};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::measure::MeasureTool;
use xtrude_app::tools::ToolContext;
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::edits::build_add_edges;

fn wall_with_window() -> Viewport {
    let mut vp = sized_viewport(1000., 700.);
    let wall = [
        v3(0., 0., 0.),
        v3(6., 0., 0.),
        v3(6., 0., 3.),
        v3(0., 0., 3.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (wall[i], wall[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    let win = [
        v3(3., 0., 1.),
        v3(4., 0., 1.),
        v3(4., 0., 2.2),
        v3(3., 0., 2.2),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (win[i], win[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    assert_eq!(vp.scene.mesh().faces.len(), 2);
    vp
}

fn measure_from_bottom_edge(vp: &mut Viewport, x: f64) {
    let m = vp.scene.mesh();
    let bottom = m
        .edge_ids()
        .into_iter()
        .map(|e| m.edge_ab(e))
        .find(|(a, b)| a.z.abs() < 1e-9 && b.z.abs() < 1e-9)
        .expect("the bottom edge");
    let mut tool = MeasureTool::new();
    tool.start_point = Some(v3(x, 0., 0.));
    tool.set_source(Some(bottom));
    vp.set_active_tool(Some(Box::new(tool)));
    // set_active_tool runs on_activate, which resets: set the state again
    vp.tool_mut::<MeasureTool>().unwrap().start_point = Some(v3(x, 0., 0.));
    vp.tool_mut::<MeasureTool>()
        .unwrap()
        .set_source(Some(bottom));
    vp.inference.axis_lock = Some(Axis::Z);
}

fn hover(vp: &mut Viewport, world: DVec3) -> SnapResult {
    let px = vp.world_to_pixel(world).expect("on screen");
    vp.process_hover(px.x, px.y, Modifiers::NONE);
    vp.inference.last_snap.clone().expect("a snap")
}

fn look(vp: &mut Viewport, yaw_deg: f64, pitch_deg: f64) {
    vp.camera.target = v3(3., 0., 1.5);
    vp.camera.distance = 10.0;
    vp.camera.yaw = yaw_deg.to_radians();
    vp.camera.pitch = pitch_deg.to_radians();
}

#[test]
fn test_the_window_corner_gives_the_locked_line_its_height() {
    let mut vp = wall_with_window();
    look(&mut vp, -60.0, -20.0); // an oblique view, not an elevation
    measure_from_bottom_edge(&mut vp, 1.0);
    let snap = hover(&mut vp, v3(3., 0., 2.2)); // the window's top-left corner
    assert_eq!(snap.kind, SnapKind::FromPoint);
    assert!((snap.point - v3(1., 0., 2.2)).length() < 1e-6);
    let (r, foot) = snap.guide.expect("the dotted guide");
    assert!((r - v3(3., 0., 2.2)).length() < 1e-6);
    assert!((foot - snap.point).length() < 1e-6);
}

#[test]
fn test_any_point_of_the_sill_is_a_reference_too() {
    let mut vp = wall_with_window();
    look(&mut vp, -60.0, -20.0);
    measure_from_bottom_edge(&mut vp, 1.0);
    let snap = hover(&mut vp, v3(3.6, 0., 1.0)); // on the sill, not a corner
    assert!(matches!(
        snap.kind,
        SnapKind::FromPoint | SnapKind::Midpoint
    ));
    assert!((snap.point.z - 1.0).abs() < 1e-6);
    assert!((snap.point.x - 1.0).abs() < 1e-6 && snap.point.y.abs() < 1e-6);
}

#[test]
fn test_away_from_any_reference_the_lock_just_projects() {
    let mut vp = wall_with_window();
    look(&mut vp, -90.0, 0.0);
    measure_from_bottom_edge(&mut vp, 1.0);
    let snap = hover(&mut vp, v3(5.2, 0., 2.6)); // bare wall
    assert_eq!(snap.kind, SnapKind::Axis);
    assert!((snap.point.x - 1.0).abs() < 1e-6);
}

#[test]
fn test_the_measure_tool_guide_lands_at_the_referenced_height() {
    let mut vp = wall_with_window();
    look(&mut vp, -60.0, -20.0);
    measure_from_bottom_edge(&mut vp, 1.0);
    let snap = hover(&mut vp, v3(3., 0., 2.2));
    let mut c = ToolContext::at(snap.point);
    c.snap = Some(snap);
    vp.with_tool(|t, vp| t.on_click(vp, &c));
    let guides = &vp.scene.doc.guides;
    assert_eq!(guides.len(), 1);
    assert!((guides[0].point.z - 2.2).abs() < 1e-6);
}
