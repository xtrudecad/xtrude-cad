// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Revolve: a face, an axis in its plane (an edge or two points), then the
//! angle (360° by default) and the sides: one closed solid, one undo step;
//! an axis through the face is refused with the reason.

use crate::common;

use common::{stub, FakeProbe, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::revolve::{RevolveTool, DEFAULT_ANGLE};
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, PickedEdge, Viewport, ViewportEvent};
use xtrude_app::window::make_tool;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

const MM: f64 = 0.001;

fn mm(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z) * MM
}

/// A 10 × 20 mm rectangle in the XZ plane, 5 mm off the Z axis.
fn profile(vp: &mut Viewport) -> FaceId {
    vp.scene.mesh_mut().add_face(
        &[
            mm(5.0, 0.0, 0.0),
            mm(15.0, 0.0, 0.0),
            mm(15.0, 0.0, 20.0),
            mm(5.0, 0.0, 20.0),
        ],
        &[],
    )
}

/// The profile selected and the tool active on it.
fn ready() -> Viewport {
    let mut vp = stub();
    let f = profile(&mut vp);
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    vp.set_active_tool(make_tool("revolve"));
    vp
}

fn click(vp: &mut Viewport, p: DVec3) {
    let c = ToolContext::at(p);
    vp.with_tool(|t, vp| {
        t.on_hover(vp, &c);
        t.on_click(vp, &c);
    });
}

fn enter(vp: &mut Viewport, typed: &str) {
    vp.set_value_buffer(typed);
    assert!(vp.handle_value_key(&KeyEvent::new(Key::Return, "", Modifiers::NONE)));
}

fn tool(vp: &Viewport) -> &RevolveTool {
    vp.tool::<RevolveTool>().unwrap()
}

fn phase(vp: &Viewport) -> &'static str {
    tool(vp).phase()
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

fn mm3(vp: &Viewport) -> f64 {
    signed_volume(vp.scene.mesh()) / (MM * MM * MM)
}

/// The volume of a rectangle `r0..r1` by `h` (mm) turned through `spans`
/// spans of `step_deg`, its corners on the stations.
fn polygon_volume(r0: f64, r1: f64, h: f64, spans: usize, step_deg: f64) -> f64 {
    h * spans as f64 * 0.5 * step_deg.to_radians().sin() * (r1 * r1 - r0 * r0)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs()
}

/// The Z axis, by two points.
fn z_axis(vp: &mut Viewport) {
    click(vp, mm(0.0, 0.0, 0.0));
    assert_eq!(phase(vp), "axis2");
    click(vp, mm(0.0, 0.0, 10.0));
    assert_eq!(phase(vp), "angle");
}

#[test]
fn test_a_full_turn_about_two_points_is_a_closed_tube() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    assert_eq!(phase(&vp), "axis");
    let depth = vp.history.undo_len();
    z_axis(&mut vp);
    // the preview: 24 spans of 4 walls
    assert_eq!(tool(&vp).preview_faces().len(), 96);
    assert_eq!(tool(&vp).angle, DEFAULT_ANGLE);
    click(&mut vp, mm(30.0, 0.0, 0.0));
    assert!(is_closed(vp.scene.mesh()));
    let v = mm3(&vp);
    let want = polygon_volume(5.0, 15.0, 20.0, 24, 15.0);
    assert!(close(v, want), "{v} vs {want}");
    let round = 200.0 * 2.0 * std::f64::consts::PI * 10.0;
    assert!((round - v) / round < 0.012, "within the 24-gon's error");
    assert_eq!(vp.history.undo_len(), depth + 1);
    assert_eq!(phase(&vp), "idle");
    vp.undo();
    assert_eq!(vp.scene.mesh().face_ids().len(), 1);
}

#[test]
fn test_a_typed_angle_turns_a_quarter_with_planar_ends() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    z_axis(&mut vp);
    enter(&mut vp, "90");
    let m = vp.scene.mesh();
    assert!(is_closed(m));
    assert!(close(mm3(&vp), polygon_volume(5.0, 15.0, 20.0, 6, 15.0)));
    for on_plane in [|q: DVec3| q.y.abs() < 1e-12, |q: DVec3| q.x.abs() < 1e-12] {
        let ends: Vec<FaceId> = m
            .face_ids()
            .into_iter()
            .filter(|&f| m.face_positions(f).into_iter().all(on_plane))
            .collect();
        assert_eq!(ends.len(), 1, "one planar end face");
    }
    // the angle is kept for the next revolve
    assert_eq!(tool(&vp).angle, 90.0);
}

#[test]
fn test_the_sides_and_angle_typed_first_are_used() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    enter(&mut vp, "48s");
    enter(&mut vp, "180");
    assert_eq!(phase(&vp), "axis", "a typed angle waits for the axis");
    z_axis(&mut vp);
    click(&mut vp, mm(30.0, 0.0, 0.0));
    assert!(is_closed(vp.scene.mesh()));
    assert!(close(mm3(&vp), polygon_volume(5.0, 15.0, 20.0, 24, 7.5)));
    assert_eq!(tool(&vp).sides, 48);
}

#[test]
fn test_an_edge_in_the_face_plane_is_the_axis() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    let (a, b) = (mm(0.0, 0.0, -5.0), mm(0.0, 0.0, 25.0));
    FakeProbe::of(&mut vp).pick_edge(move |_x, _y| {
        Some(PickedEdge {
            a,
            b,
            edge: None,
            in_group: false,
            context: None,
            group: None,
            soft: false,
            curve: None,
        })
    });
    click(&mut vp, mm(0.0, 0.0, 3.0));
    assert_eq!(phase(&vp), "angle");
    let (o, d) = tool(&vp).axis.unwrap();
    assert!(o.x.abs() < 1e-12 && o.y.abs() < 1e-12 && d.z.abs() > 1.0 - 1e-12);
    click(&mut vp, mm(30.0, 0.0, 0.0));
    assert!(close(mm3(&vp), polygon_volume(5.0, 15.0, 20.0, 24, 15.0)));
}

#[test]
fn test_an_edge_off_the_face_plane_is_a_first_point() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    let (a, b) = (mm(0.0, 5.0, 0.0), mm(0.0, 5.0, 10.0));
    FakeProbe::of(&mut vp).pick_edge(move |_x, _y| {
        Some(PickedEdge {
            a,
            b,
            edge: None,
            in_group: false,
            context: None,
            group: None,
            soft: false,
            curve: None,
        })
    });
    // a point snapped off the plane is brought onto it
    click(&mut vp, mm(0.0, 5.0, 0.0));
    assert_eq!(phase(&vp), "axis2");
    assert!(tool(&vp).axis_start.unwrap().y.abs() < 1e-12);
}

#[test]
fn test_an_axis_through_the_face_is_refused() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    let depth = vp.history.undo_len();
    click(&mut vp, mm(10.0, 0.0, 0.0));
    click(&mut vp, mm(10.0, 0.0, 10.0));
    assert_eq!(phase(&vp), "axis", "back to picking the axis");
    assert!(tool(&vp).preview_faces().is_empty());
    assert!(flashes(&mut vp)
        .iter()
        .any(|f| f.contains("runs through the face")));
    assert_eq!(vp.history.undo_len(), depth);
    // a good axis still turns it
    z_axis(&mut vp);
    click(&mut vp, mm(30.0, 0.0, 0.0));
    assert!(is_closed(vp.scene.mesh()));
}

#[test]
fn test_an_angle_beyond_a_full_turn_is_refused() {
    let _m = MetreDocument::new();
    let mut vp = ready();
    z_axis(&mut vp);
    let depth = vp.history.undo_len();
    enter(&mut vp, "400");
    assert_eq!(vp.history.undo_len(), depth);
    assert_eq!(phase(&vp), "angle");
    assert!(flashes(&mut vp).iter().any(|f| f.contains("at most 360")));
    assert_eq!(tool(&vp).angle, DEFAULT_ANGLE);
}

#[test]
fn test_a_face_is_clicked_or_asked_for() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let f = profile(&mut vp);
    vp.set_active_tool(make_tool("revolve"));
    assert_eq!(phase(&vp), "idle");
    click(&mut vp, DVec3::ZERO);
    assert_eq!(phase(&vp), "idle");
    assert!(flashes(&mut vp)
        .iter()
        .any(|t| t.contains("click the face")));
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
    click(&mut vp, mm(10.0, 0.0, 10.0));
    assert_eq!(phase(&vp), "axis");
    assert_eq!(tool(&vp).face, Some(f));
    // the cursor is read on the face's plane
    let plane = tool(&vp).drag_plane(&vp).unwrap();
    assert!(plane.1.y.abs() > 1.0 - 1e-12);
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert_eq!(phase(&vp), "idle");
}
