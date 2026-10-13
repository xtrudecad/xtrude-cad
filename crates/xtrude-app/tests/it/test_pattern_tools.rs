// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Linear and Circular Pattern: the selection repeated along a direction
//! or around an axis, by count and spacing, in one undo step; groups
//! copied as Move's copy does (a component instance stays an instance).

use crate::common;

use common::{add_box, stub, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::pattern::{CircularPatternTool, LinearPatternTool, DEFAULT_COUNT};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::value_box::PatternEntry;
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_app::window::make_tool;
use xtrude_model::commands::MakeGroupCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::{GroupId, MeshId};

const MM: f64 = 0.001;

fn mm(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z) * MM
}

fn click(vp: &mut Viewport, p: DVec3) {
    let c = ToolContext::at(p);
    vp.with_tool(|t, vp| {
        t.on_hover(vp, &c);
        t.on_click(vp, &c);
    });
}

fn hover(vp: &mut Viewport, p: DVec3) {
    vp.with_tool(|t, vp| t.on_hover(vp, &ToolContext::at(p)));
}

/// `typed` + Enter in the value box.
fn enter(vp: &mut Viewport, typed: &str) {
    vp.set_value_buffer(typed);
    assert!(vp.handle_value_key(&KeyEvent::new(Key::Return, "", Modifiers::NONE)));
}

fn flashes(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(text, _) => Some(text),
            _ => None,
        })
        .collect()
}

/// A 10 mm cube at the origin, made a group (or a component) and selected.
fn cube_group(vp: &mut Viewport, component: bool) -> GroupId {
    add_box(vp, DVec3::ZERO, mm(10.0, 10.0, 10.0));
    let m = vp.scene.mesh();
    let cmd = MakeGroupCommand::new(m, &m.face_ids(), &m.edge_ids(), component, None);
    vp.run(cmd);
    let g = vp.scene.doc.groups[0].id;
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Group(g));
    g
}

/// The lowest corner of every top-level group, in mm, by X.
fn group_corners(vp: &Viewport) -> Vec<DVec3> {
    let mut out: Vec<DVec3> = vp
        .scene
        .doc
        .groups
        .iter()
        .map(|g| {
            let m = vp.scene.doc.mesh(g.mesh);
            m.vertex_ids()
                .into_iter()
                .map(|v| {
                    let p = m.pos(v);
                    g.xform.map_or(p, |x| x.transform_point3(p))
                })
                .fold(DVec3::splat(f64::MAX), DVec3::min)
                / MM
        })
        .collect();
    out.sort_by(|a, b| a.x.total_cmp(&b.x));
    out
}

fn xs(corners: &[DVec3]) -> Vec<f64> {
    corners.iter().map(|c| (c.x * 1e6).round() / 1e6).collect()
}

fn linear(vp: &mut Viewport) {
    vp.set_active_tool(make_tool("pattern_linear"));
}

#[test]
fn test_a_linear_pattern_of_a_cube_four_by_15_mm() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    cube_group(&mut vp, false);
    linear(&mut vp);
    let depth = vp.history.undo_len();
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(5.0, 0.0, 0.0));
    enter(&mut vp, "4;15mm");
    let corners = group_corners(&vp);
    assert_eq!(xs(&corners), vec![0.0, 15.0, 30.0, 45.0]);
    assert!(corners.iter().all(|c| c.y.abs() < 1e-9 && c.z.abs() < 1e-9));
    assert_eq!(vp.history.undo_len(), depth + 1);
    vp.undo();
    assert_eq!(vp.scene.doc.groups.len(), 1);
}

#[test]
fn test_a_linear_pattern_divides_a_span_equally() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    cube_group(&mut vp, false);
    linear(&mut vp);
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(0.0, 1.0, 0.0));
    enter(&mut vp, "4/45mm");
    let ys: Vec<f64> = group_corners(&vp)
        .iter()
        .map(|c| (c.y * 1e6).round() / 1e6)
        .collect();
    let mut ys = ys;
    ys.sort_by(f64::total_cmp);
    assert_eq!(ys, vec![0.0, 15.0, 30.0, 45.0]);
}

#[test]
fn test_the_second_click_sets_the_spacing_and_a_retype_lays_it_again() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    cube_group(&mut vp, false);
    linear(&mut vp);
    let depth = vp.history.undo_len();
    click(&mut vp, DVec3::ZERO);
    click(&mut vp, mm(20.0, 0.0, 0.0));
    assert_eq!(DEFAULT_COUNT, 3);
    assert_eq!(xs(&group_corners(&vp)), vec![0.0, 20.0, 40.0]);
    // right after: a count alone keeps the spacing, in the same step
    enter(&mut vp, "5");
    assert_eq!(xs(&group_corners(&vp)), vec![0.0, 20.0, 40.0, 60.0, 80.0]);
    // `count/` spreads the same span anew
    enter(&mut vp, "3/");
    assert_eq!(xs(&group_corners(&vp)), vec![0.0, 40.0, 80.0]);
    assert_eq!(vp.history.undo_len(), depth + 1);
    // the count is kept for the next pattern
    assert_eq!(vp.tool::<LinearPatternTool>().unwrap().count, 3);
}

#[test]
fn test_a_live_preview_shows_every_copy() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    cube_group(&mut vp, false);
    linear(&mut vp);
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(15.0, 0.0, 0.0));
    let t = vp.tool::<LinearPatternTool>().unwrap();
    // the direction, then the cube's 12 edges for each of the 2 copies
    let lines = t.rubber_band_lines(&vp);
    assert_eq!(lines.len(), 1 + 2 * 12);
    let far = lines.iter().map(|(a, b)| a.x.max(b.x)).fold(0.0, f64::max);
    assert!((far / MM - 40.0).abs() < 1e-9, "{far}");
    let (label, _) = t.value_label(&vp).unwrap();
    assert!(label.starts_with("3 ×"), "{label}");
}

#[test]
fn test_a_pattern_of_a_component_lays_instances() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let g = cube_group(&mut vp, true);
    let def = vp.scene.doc.find_group(g).unwrap().mesh;
    assert!(vp.scene.doc.find_group(g).unwrap().component);
    linear(&mut vp);
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(1.0, 0.0, 0.0));
    enter(&mut vp, "3;15mm");
    assert_eq!(xs(&group_corners(&vp)), vec![0.0, 15.0, 30.0]);
    for g in &vp.scene.doc.groups {
        assert!(g.component, "every copy is an instance");
        assert_eq!(g.mesh, def, "of the one definition");
    }
}

#[test]
fn test_a_pattern_of_loose_faces_copies_the_faces() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    add_box(&mut vp, DVec3::ZERO, mm(10.0, 10.0, 10.0));
    let mid = vp.scene.active_mesh_id();
    let faces = vp.scene.mesh().face_ids();
    vp.scene
        .selection
        .extend(faces.iter().map(|&f| EntityRef::Face(mid, f)));
    linear(&mut vp);
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(1.0, 0.0, 0.0));
    enter(&mut vp, "4;15mm");
    let m = vp.scene.mesh();
    assert_eq!(m.face_ids().len(), 4 * 6);
    let mut centre_xs: Vec<f64> = m
        .face_ids()
        .into_iter()
        .map(|f| (m.face_centroid(f).x / MM * 1e6).round() / 1e6)
        .collect();
    centre_xs.sort_by(f64::total_cmp);
    centre_xs.dedup();
    // each cube's faces at x = 15k (its low side), 15k + 5, 15k + 10
    let want: Vec<f64> = (0..12).map(|i| 5.0 * i as f64).collect();
    assert_eq!(centre_xs, want);
}

#[test]
fn test_a_pattern_needs_something_to_repeat_and_two_instances() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    linear(&mut vp);
    click(&mut vp, DVec3::ZERO);
    assert!(vp.tool::<LinearPatternTool>().unwrap().start.is_none());
    assert!(flashes(&mut vp).iter().any(|f| f.contains("to repeat")));
    cube_group(&mut vp, false);
    let depth = vp.history.undo_len();
    click(&mut vp, DVec3::ZERO);
    hover(&mut vp, mm(1.0, 0.0, 0.0));
    enter(&mut vp, "1;15mm");
    assert_eq!(vp.history.undo_len(), depth);
    assert!(flashes(&mut vp).iter().any(|f| f.contains("2 to")));
}

// ---- Circular Pattern -------------------------------------------------------

/// A little square on the +X axis at (2, 0), selected.
fn marker(vp: &mut Viewport) {
    let h = 0.1;
    let f = vp.scene.mesh_mut().add_face(
        &[
            DVec3::new(2.0 - h, -h, 0.),
            DVec3::new(2.0 + h, -h, 0.),
            DVec3::new(2.0 + h, h, 0.),
            DVec3::new(2.0 - h, h, 0.),
        ],
        &[],
    );
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
}

/// Angle (degrees, 0–360) of each face's centroid about the origin.
fn angles(vp: &Viewport) -> Vec<f64> {
    let m = vp.scene.mesh();
    let mut out: Vec<f64> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let c = m.face_centroid(f);
            (c.y.atan2(c.x).to_degrees().rem_euclid(360.0) * 1000.0).round() / 1000.0
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

fn circular(vp: &mut Viewport) {
    vp.set_active_tool(make_tool("pattern_circular"));
}

#[test]
fn test_a_circular_pattern_six_by_60_degrees_about_z() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    marker(&mut vp);
    circular(&mut vp);
    let depth = vp.history.undo_len();
    click(&mut vp, DVec3::ZERO);
    enter(&mut vp, "6;60");
    assert_eq!(angles(&vp), vec![0.0, 60.0, 120.0, 180.0, 240.0, 300.0]);
    assert_eq!(vp.history.undo_len(), depth + 1);
    // every copy stays 2 from the axis
    let m = vp.scene.mesh();
    for f in m.face_ids() {
        let c = m.face_centroid(f);
        assert!((c.truncate().length() - 2.0).abs() < 1e-9 && c.z.abs() < 1e-12);
    }
}

#[test]
fn test_a_count_alone_shares_the_full_turn() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    marker(&mut vp);
    circular(&mut vp);
    click(&mut vp, DVec3::ZERO);
    enter(&mut vp, "6");
    assert_eq!(angles(&vp), vec![0.0, 60.0, 120.0, 180.0, 240.0, 300.0]);
    // retyped right after: the same step, laid again
    let depth = vp.history.undo_len();
    enter(&mut vp, "4");
    assert_eq!(angles(&vp), vec![0.0, 90.0, 180.0, 270.0]);
    assert_eq!(vp.history.undo_len(), depth);
    // a span spread over: its ends included
    enter(&mut vp, "4/90");
    assert_eq!(angles(&vp), vec![0.0, 30.0, 60.0, 90.0]);
}

#[test]
fn test_a_swept_angle_sets_the_step() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    marker(&mut vp);
    circular(&mut vp);
    click(&mut vp, DVec3::ZERO);
    // before an arm, the preview shares the full turn
    let t = vp.tool::<CircularPatternTool>().unwrap();
    assert_eq!(t.value_label(&vp).unwrap().0, "3 × 120.0°");
    click(&mut vp, DVec3::new(2.0, 0.0, 0.0));
    let a = 45f64.to_radians();
    click(&mut vp, DVec3::new(2.0 * a.cos(), 2.0 * a.sin(), 0.0));
    assert_eq!(angles(&vp), vec![0.0, 45.0, 90.0]);
}

#[test]
fn test_the_tools_take_patterns_only() {
    let t = LinearPatternTool::new();
    assert!(t.value_spec().pattern && !t.value_spec().unitless);
    let c = CircularPatternTool::new();
    assert!(c.value_spec().pattern && c.value_spec().unitless);
    let mut vp = stub();
    marker(&mut vp);
    circular(&mut vp);
    click(&mut vp, DVec3::ZERO);
    let took = vp
        .with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(3.0)))
        .unwrap();
    assert!(!took);
    let took = vp
        .with_tool(|t, vp| {
            t.on_value(
                vp,
                &ToolValue::Pattern(PatternEntry {
                    count: 2,
                    step: None,
                    spread: false,
                }),
            )
        })
        .unwrap();
    assert!(took);
    assert_eq!(angles(&vp), vec![0.0, 180.0]);
}
