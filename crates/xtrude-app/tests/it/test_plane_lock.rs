// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Arrow keys before the first click lock a planar tool's drawing plane; Down
//! locks it to the hovered edge (perpendicular) or face (parallel).

use crate::common;
use crate::common::FakeProbe;
use xtrude_app::input::ScreenPt;
use xtrude_core::Axis;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::arc::CenterArcTool;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::{RefKind, Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, PickedEdge, Viewport, ViewportEvent};
use xtrude_model::ids::MeshId;

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

fn key(t: &mut dyn Tool, vp: &mut Viewport, k: Key) -> bool {
    t.on_key(vp, k, Modifiers::NONE)
}

fn edge(a: DVec3, b: DVec3) -> PickedEdge {
    PickedEdge {
        a,
        b,
        edge: None,
        in_group: false,
        context: None,
        group: None,
        soft: false,
        curve: None,
    }
}

/// The flashed status messages, oldest first.
fn flashed(vp: &Viewport) -> Vec<String> {
    vp.outbox
        .events
        .iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t.to_lowercase()),
            _ => None,
        })
        .collect()
}

fn last_flash(vp: &Viewport) -> String {
    flashed(vp).pop().unwrap_or_default()
}

fn last_face_positions(vp: &Viewport) -> Vec<DVec3> {
    let m = vp.scene.mesh();
    let f = *m.face_ids().last().expect("a face");
    m.face_positions(f)
}

#[test]
fn test_left_arrow_locks_a_circle_to_the_xz_plane_and_the_shape_spends_it() {
    let mut vp = stub();
    let mut tool = RadialTool::circle();
    tool.on_activate(&mut vp);
    assert!(key(&mut tool, &mut vp, Key::Left));
    assert_eq!(tool.lock.plane_lock, Some(Axis::Y));
    assert!(last_flash(&vp).contains("xz"));
    tool.on_click(&mut vp, &ctx(v3(1., 2., 3.))); // the centre
    assert_eq!(tool.work_plane.unwrap().1, DVec3::Y);
    // after the first click the arrows are not the tool's business
    assert!(!key(&mut tool, &mut vp, Key::Up));
    tool.on_click(&mut vp, &ctx(v3(2.5, 2., 3.))); // the rim
    assert_eq!(vp.scene.mesh().face_ids().len(), 1);
    let ps = last_face_positions(&vp);
    assert!(ps.iter().all(|p| (p.y - 2.0).abs() < 1e-9));
    assert!(ps.iter().any(|p| (p.z - 3.0).abs() > 1e-6));
    assert!(tool.lock.plane_lock.is_none() && tool.work_plane.is_none()); // spent
}

#[test]
fn test_same_arrow_again_frees_and_right_up_pick_the_other_planes() {
    let mut vp = stub();
    let mut tool = RadialTool::polygon();
    tool.on_activate(&mut vp);
    key(&mut tool, &mut vp, Key::Right);
    assert_eq!(tool.lock.plane_lock, Some(Axis::X));
    key(&mut tool, &mut vp, Key::Right);
    assert!(tool.lock.plane_lock.is_none());
    let f = last_flash(&vp);
    assert!(f.contains("free") || f.contains("libre"));
    key(&mut tool, &mut vp, Key::Up);
    assert_eq!(tool.lock.plane_lock, Some(Axis::Z));
    assert!(key(&mut tool, &mut vp, Key::Down)); // Down is ours too
    assert_eq!(tool.lock.plane_lock, Some(Axis::Z)); // (nothing hovered: no-op)
    tool.on_cancel(&mut vp);
    assert!(tool.lock.plane_lock.is_none()); // Esc releases it
}

#[test]
fn test_rectangle_and_centre_arc_lock_too() {
    let mut vp = stub();
    let mut rect = RectangleTool::new();
    rect.on_activate(&mut vp);
    key(&mut rect, &mut vp, Key::Right); // normal X → YZ
    rect.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    rect.on_click(&mut vp, &ctx(v3(0., 3., 2.)));
    let ps = last_face_positions(&vp);
    assert!(ps.iter().all(|p| p.x.abs() < 1e-9));
    let mut zs: Vec<i64> = ps.iter().map(|p| (p.z * 1e6).round() as i64).collect();
    zs.sort();
    zs.dedup();
    assert_eq!(zs.len(), 2);
    let mut arc = CenterArcTool::new();
    arc.on_activate(&mut vp);
    key(&mut arc, &mut vp, Key::Left);
    arc.on_click(&mut vp, &ctx(v3(5., 5., 5.)));
    assert_eq!(arc.work_plane, Some((v3(5., 5., 5.), DVec3::Y)));
}

fn perp_to(n: DVec3, c: DVec3, ps: &[DVec3]) -> bool {
    ps.iter().all(|p| (*p - c).dot(n).abs() < 1e-6)
}

#[test]
fn test_down_over_an_inclined_edge_draws_the_circle_perpendicular_to_it() {
    let mut vp = stub();
    let mut tool = RadialTool::circle();
    tool.on_activate(&mut vp);
    vp.inference.hover_edge = Some(edge(DVec3::ZERO, v3(3., 0., 4.))); // a 53° axis line
    assert!(key(&mut tool, &mut vp, Key::Down));
    let (n, kind) = tool.lock.plane_ref.unwrap();
    assert!(kind == RefKind::Edge && (n.x - 0.6).abs() < 1e-6 && (n.z - 0.8).abs() < 1e-6);
    assert!(last_flash(&vp).contains("perpendicular"));
    assert_eq!(tool.lock.lock_color(), Some([0.85, 0.30, 0.80, 1.0])); // magenta
    tool.on_click(&mut vp, &ctx(v3(3., 0., 4.))); // centre on the line's end
    assert_eq!(tool.work_plane.unwrap().1, n);
    tool.on_click(&mut vp, &ctx(v3(3., 1., 4.))); // the rim (Y is in-plane)
    assert_eq!(vp.scene.mesh().face_ids().len(), 1);
    let ps = last_face_positions(&vp);
    assert!(perp_to(n, v3(3., 0., 4.), &ps));
    let r = ps
        .iter()
        .map(|p| (*p - v3(3., 0., 4.)).length())
        .fold(0.0, f64::max);
    assert!((r - 1.0).abs() < 1e-6);
    assert!(tool.lock.plane_ref.is_none() && tool.work_plane.is_none()); // spent
}

#[test]
fn test_down_again_frees_and_an_arrow_replaces_the_reference() {
    let mut vp = stub();
    let mut tool = RadialTool::polygon();
    tool.on_activate(&mut vp);
    assert!(key(&mut tool, &mut vp, Key::Down)); // nothing hovered:
    assert!(tool.lock.plane_ref.is_none() && flashed(&vp).is_empty()); // ours, no-op
    vp.inference.hover_edge = Some(edge(DVec3::ZERO, v3(0., 2., 0.)));
    key(&mut tool, &mut vp, Key::Down);
    assert_eq!(tool.lock.plane_ref.unwrap().0, DVec3::Y);
    key(&mut tool, &mut vp, Key::Down);
    assert!(tool.lock.plane_ref.is_none());
    let f = last_flash(&vp);
    assert!(f.contains("free") || f.contains("libre"));
    key(&mut tool, &mut vp, Key::Down);
    key(&mut tool, &mut vp, Key::Up); // an axis wins
    assert!(tool.lock.plane_ref.is_none() && tool.lock.plane_lock == Some(Axis::Z));
    key(&mut tool, &mut vp, Key::Down); // and vice versa
    assert!(tool.lock.plane_lock.is_none() && tool.lock.plane_ref.unwrap().1 == RefKind::Edge);
    tool.on_cancel(&mut vp);
    assert!(tool.lock.plane_ref.is_none()); // Esc releases it
}

#[test]
fn test_down_over_a_face_locks_its_plane_and_the_edge_beats_the_face() {
    let mut vp = stub();
    let mut rect = RectangleTool::new();
    rect.on_activate(&mut vp);
    key(&mut rect, &mut vp, Key::Right); // a YZ wall
    rect.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    rect.on_click(&mut vp, &ctx(v3(0., 3., 2.)));
    let wall = *vp.scene.mesh().face_ids().last().unwrap();
    vp.input.last_mouse_pos = Some(ScreenPt::new(10., 10.));
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: wall,
        placement: None,
        xform: None,
        owner: None,
    }));
    let mut tool = RadialTool::circle();
    tool.on_activate(&mut vp);
    key(&mut tool, &mut vp, Key::Down);
    let (n, kind) = tool.lock.plane_ref.unwrap();
    assert!(kind == RefKind::Face && (n.x.abs() - 1.0).abs() < 1e-9);
    let f = last_flash(&vp);
    assert!(f.contains("parallel"));
    tool.on_click(&mut vp, &ctx(v3(5., 5., 5.))); // off the wall, same plane
    assert!((tool.work_plane.unwrap().1.x.abs() - 1.0).abs() < 1e-9);
    tool.on_cancel(&mut vp);
    vp.inference.hover_edge = Some(edge(DVec3::ZERO, v3(0., 0., 1.))); // an edge too
    key(&mut tool, &mut vp, Key::Down);
    let r = tool.lock.plane_ref.unwrap();
    assert!(r.1 == RefKind::Edge && r.0 == DVec3::Z);
}
