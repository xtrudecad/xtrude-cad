// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The sweep engine and the tool's flows.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::sweep::SweepTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, PickedEdge, Viewport, ViewportEvent};
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::sweep::{manual_path_extend, order_path_edges, sweep_profile};
use xtrude_core::{EdgeId, FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn square_profile_xz(m: &mut Mesh) -> FaceId {
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 0., 1.),
            v3(0., 0., 1.),
        ],
        &[],
    )
}

#[test]
fn test_straight_sweep_is_a_box() {
    let mut m = Mesh::new();
    let f = square_profile_xz(&mut m);
    assert!(sweep_profile(
        &mut m,
        f,
        &[v3(0., 0., 0.), v3(0., 3., 0.)],
        false
    ));
    assert!(is_closed(&m));
    assert!((signed_volume(&m) - 3.0).abs() < 1e-6);
    assert_eq!(m.faces.len(), 6);
}

#[test]
fn test_l_path_mitred_corner() {
    let mut m = Mesh::new();
    let f = square_profile_xz(&mut m);
    assert!(sweep_profile(
        &mut m,
        f,
        &[v3(0., 0., 0.), v3(0., 4., 0.), v3(4., 4., 0.)],
        false
    ));
    assert!(is_closed(&m));
    let vol = signed_volume(&m);
    assert!(6.5 < vol && vol < 8.5);
    assert!(m
        .verts
        .iter()
        .any(|(_, v)| (v.position.y - 4.0 + v.position.x).abs() < 1e-6));
}

#[test]
fn test_closed_path_makes_a_welded_torus() {
    let mut m = Mesh::new();
    let f = m.add_face(
        &[
            v3(4., 0., 0.),
            v3(5., 0., 0.),
            v3(5., 0., 1.),
            v3(4., 0., 1.),
        ],
        &[],
    );
    let n = 12;
    let ring: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            v3(4.5 * a.cos(), 4.5 * a.sin(), 0.5)
        })
        .collect();
    assert!(sweep_profile(&mut m, f, &ring, true));
    assert!(is_closed(&m));
    assert_eq!(m.faces.len(), 4 * n);
    assert!(signed_volume(&m) > 0.0);
    assert!(m.edge_ids().into_iter().any(|e| m.edge(e).soft));
}

#[test]
fn test_holed_profile_sweeps_a_tube() {
    let mut m = Mesh::new();
    let outer = [
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 0., 2.),
        v3(0., 0., 2.),
    ];
    let hole = vec![
        v3(0.5, 0., 0.5),
        v3(1.5, 0., 0.5),
        v3(1.5, 0., 1.5),
        v3(0.5, 0., 1.5),
    ];
    let f = m.add_face(&outer, &[hole]);
    assert!(sweep_profile(
        &mut m,
        f,
        &[v3(0., 0., 0.), v3(0., 5., 0.)],
        false
    ));
    assert!(is_closed(&m));
    assert!((signed_volume(&m) - 15.0).abs() < 1e-6);
    assert_eq!(
        m.face_ids()
            .into_iter()
            .filter(|&g| !m.face(g).holes.is_empty())
            .count(),
        2
    );
}

#[test]
fn test_order_path_edges_chains_and_rejects_branches() {
    let mut m = Mesh::new();
    m.add_edge(v3(0., 0., 0.), v3(1., 0., 0.)).unwrap();
    m.add_edge(v3(2., 0., 0.), v3(1., 0., 0.)).unwrap();
    m.add_edge(v3(2., 0., 0.), v3(2., 1., 0.)).unwrap();
    let (pts, closed) = order_path_edges(&m, &m.edge_ids()).unwrap();
    assert!(!closed);
    let first = (
        (pts[0].x * 1000.0).round() as i64,
        (pts[0].y * 1000.0).round() as i64,
    );
    assert!((first == (0, 0) || first == (2000, 1000)) && pts.len() == 4);
    m.add_edge(v3(1., 0., 0.), v3(1., 1., 0.)).unwrap();
    assert!(order_path_edges(&m, &m.edge_ids()).is_none());
}

#[test]
fn test_reversal_path_declines_cleanly() {
    let mut m = Mesh::new();
    let f = square_profile_xz(&mut m);
    let before = m.faces.len();
    assert!(!sweep_profile(
        &mut m,
        f,
        &[v3(0., 0., 0.), v3(0., 3., 0.), v3(0., 0., 0.)],
        false
    ));
    assert_eq!(m.faces.len(), before);
}

fn pick(vp: &mut Viewport, f: Option<FaceId>) {
    FakeProbe::of(vp).pick(f.map(|face| FaceHit {
        mesh: MeshId::LOOSE,
        face,
        placement: None,
        xform: None,
        owner: None,
    }));
}

fn pick_edge(vp: &mut Viewport, e: Option<EdgeId>) {
    let edge = e.map(|e| {
        let (a, b) = vp.scene.mesh().edge_ab(e);
        PickedEdge {
            a,
            b,
            edge: Some((MeshId::LOOSE, e)),
            in_group: false,
            context: None,
            group: None,
            soft: false,
            curve: None,
        }
    });
    FakeProbe::of(vp).pick_edge(move |_x, _y| edge.clone());
}

fn ctx(x: f64, y: f64, world: DVec3, mods: Modifiers) -> ToolContext {
    let mut c = ToolContext::at(world).with_modifiers(mods);
    c.screen = (x, y);
    c
}

fn c0() -> ToolContext {
    ctx(0., 0., DVec3::ZERO, Modifiers::NONE)
}

fn messages(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn test_tool_flow_path_then_profile() {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let profile = square_profile_xz(m);
    let e1 = m.add_edge(v3(0., 0., 0.), v3(0., 4., 0.)).unwrap();
    let e2 = m.add_edge(v3(0., 4., 0.), v3(4., 4., 0.)).unwrap();
    vp.scene
        .selection
        .insert(EntityRef::Edge(MeshId::LOOSE, e1));
    vp.scene
        .selection
        .insert(EntityRef::Edge(MeshId::LOOSE, e2));
    let mut t = SweepTool::new();
    t.on_activate(&mut vp);
    assert!(t.path.is_some());
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &c0());
    assert!(is_closed(vp.scene.mesh()));
    assert!(signed_volume(vp.scene.mesh()) > 0.0);
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_manual_drag_previews_live_and_a_click_finishes() {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let profile = square_profile_xz(m);
    let e1 = m.add_edge(v3(0., 0., 0.), v3(0., 4., 0.)).unwrap();
    let e2 = m.add_edge(v3(0., 4., 0.), v3(4., 4., 0.)).unwrap();
    let mut t = SweepTool::new();
    vp.take_events();
    t.on_activate(&mut vp);
    assert!(t.path.is_none() && !messages(&mut vp).is_empty());
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &c0());
    t.on_release(&mut vp);
    assert!(t.profile == Some(profile) && t.preview_faces().is_empty());
    pick(&mut vp, None);
    pick_edge(&mut vp, Some(e1));
    t.on_hover(&mut vp, &ctx(30., 0., v3(0., 2., 0.), Modifiers::NONE));
    let chain: Vec<DVec3> = t.chain.iter().map(|&v| vp.scene.mesh().pos(v)).collect();
    assert_eq!(chain, vec![v3(0., 0., 0.), v3(0., 4., 0.)]);
    assert_eq!(t.rubber_band_lines(&vp).len(), 1);
    assert_eq!(t.preview_faces().len(), 5);
    pick_edge(&mut vp, Some(e2));
    t.on_hover(&mut vp, &ctx(60., 0., v3(2., 4., 0.), Modifiers::NONE));
    assert!(t.chain.len() == 3 && t.rubber_band_lines(&vp).len() == 2);
    assert_eq!(t.preview_faces().len(), 9);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    t.on_click(&mut vp, &ctx(60., 0., DVec3::ZERO, Modifiers::NONE));
    assert!(is_closed(vp.scene.mesh()));
    let vol = signed_volume(vp.scene.mesh());
    assert!(6.5 < vol && vol < 8.5);
    assert!(t.profile.is_none() && t.preview_faces().is_empty());
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_manual_drag_bridges_skipped_segments_and_backs_up() {
    let mut m = Mesh::new();
    let pts: Vec<DVec3> = (0..5).map(|i| v3(0., i as f64, 0.)).collect();
    let edges: Vec<EdgeId> = (0..4)
        .map(|i| m.add_edge(pts[i], pts[i + 1]).unwrap())
        .collect();
    let v: Vec<_> = pts.iter().map(|p| m.vertex_at(*p).unwrap()).collect();
    let never = |_e: EdgeId| false;
    let mut chain = vec![v[0], v[1]];
    assert!(manual_path_extend(&m, &mut chain, edges[2], &never, 400));
    assert_eq!(chain, vec![v[0], v[1], v[2], v[3]]);
    assert!(manual_path_extend(&m, &mut chain, edges[1], &never, 400));
    assert_eq!(chain, vec![v[0], v[1], v[2]]);
    assert!(manual_path_extend(&m, &mut chain, edges[1], &never, 400));
    assert_eq!(chain, vec![v[0], v[1]]);
    assert!(
        manual_path_extend(&m, &mut chain, edges[1], &never, 400)
            && chain == vec![v[0], v[1], v[2]]
    );
    assert!(!manual_path_extend(
        &m,
        &mut chain,
        edges[1],
        &|_e| true,
        400
    ));
}

#[test]
fn test_manual_drag_release_after_a_real_drag_finishes_and_esc_cancels() {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let profile = square_profile_xz(m);
    let e1 = m.add_edge(v3(0., 0., 0.), v3(0., 3., 0.)).unwrap();
    let mut t = SweepTool::new();
    t.on_activate(&mut vp);
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &ctx(10., 10., DVec3::ZERO, Modifiers::NONE));
    pick(&mut vp, None);
    pick_edge(&mut vp, Some(e1));
    t.on_hover(&mut vp, &ctx(12., 11., v3(0., 1., 0.), Modifiers::NONE));
    t.on_release(&mut vp);
    assert!(t.profile == Some(profile) && vp.scene.mesh().faces.len() == 1);
    t.on_cancel(&mut vp);
    assert!(t.profile.is_none() && t.preview_faces().is_empty());
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &ctx(10., 10., DVec3::ZERO, Modifiers::NONE));
    pick(&mut vp, None);
    t.on_hover(&mut vp, &ctx(80., 10., v3(0., 2., 0.), Modifiers::NONE));
    t.on_release(&mut vp);
    assert!(is_closed(vp.scene.mesh()) && (signed_volume(vp.scene.mesh()) - 3.0).abs() < 1e-6);
}

#[test]
fn test_alt_over_a_face_follows_its_perimeter() {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let path_face = m.add_face(
        &[
            v3(0., 0., 0.),
            v3(6., 0., 0.),
            v3(6., 6., 0.),
            v3(0., 6., 0.),
        ],
        &[],
    );
    let profile = m.add_face(
        &[
            v3(0., 0., 0.5),
            v3(-1., 0., 0.5),
            v3(-1., 0., 1.5),
            v3(0., 0., 1.5),
        ],
        &[],
    );
    let mut t = SweepTool::new();
    t.on_activate(&mut vp);
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &c0());
    pick(&mut vp, Some(path_face));
    t.on_hover(&mut vp, &ctx(40., 40., DVec3::ZERO, Modifiers::ALT));
    let (pts, closed) = t.path_points(&vp);
    assert!(closed && pts.len() == 4 && t.preview_faces().len() == 16);
    assert_eq!(t.rubber_band_lines(&vp).len(), 4);
    t.on_click(&mut vp, &ctx(40., 40., DVec3::ZERO, Modifiers::ALT));
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1 + 16);
    assert!(m.edge_ids().into_iter().all(|e| {
        let (a, b) = m.edge_ab(e);
        !(a.z > 1e-6 || b.z > 1e-6) || m.edge(e).faces.len() == 2
    }));
    assert!(vp.undo() && vp.scene.mesh().faces.len() == 2);
}

#[test]
fn test_edges_in_the_profile_plane_are_not_path_edges() {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let profile = square_profile_xz(m);
    let in_plane = m.add_edge(v3(2., 0., 0.), v3(2., 0., 2.)).unwrap();
    let mut t = SweepTool::new();
    t.on_activate(&mut vp);
    pick(&mut vp, Some(profile));
    t.on_hover(&mut vp, &c0());
    t.on_click(&mut vp, &c0());
    pick(&mut vp, None);
    pick_edge(&mut vp, Some(in_plane));
    t.on_hover(&mut vp, &ctx(30., 0., DVec3::ZERO, Modifiers::NONE));
    assert!(t.chain.is_empty() && t.preview_faces().is_empty());
    vp.take_events();
    t.on_click(&mut vp, &ctx(30., 0., DVec3::ZERO, Modifiers::NONE));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(messages(&mut vp)
        .last()
        .is_some_and(|m| m.starts_with("Sweep: touch")));
}

#[test]
fn test_sweep_tool_is_registered_as_sweep() {
    let tool = xtrude_app::window::make_tool("sweep").expect("no tool under \"sweep\"");
    let info = tool.info();
    assert_eq!(info.key, "sweep");
    assert_eq!(info.name, "Sweep");
    assert_eq!(info.icon, Some("sweep"));
    assert_eq!(info.shortcut, Some("W"));
    assert!(
        xtrude_app::shell::icons::render("sweep", 1.0, xtrude_app::shell::icons::ACCENT).is_some()
    );
    let model = xtrude_app::window::TOOLBAR_LAYOUT
        .iter()
        .find(|(id, _, _)| *id == "model")
        .expect("no Model toolbar");
    assert!(model.2.contains(&"sweep"));
    // The old id is gone: tool ids are not persisted, so nothing reads it.
    assert!(xtrude_app::window::make_tool("followme").is_none());
    assert!(
        xtrude_app::shell::icons::render("followme", 1.0, xtrude_app::shell::icons::ACCENT)
            .is_none()
    );
    assert!(!xtrude_app::window::TOOLBAR_LAYOUT
        .iter()
        .any(|(_, _, keys)| keys.contains(&"followme")));
}
