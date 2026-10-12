// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Ctrl = push/pull a copy, double-click repeats the last distance, typed
//! negatives reverse, the inward clamp, the refusal guard, distance inference,
//! the drag preview, paint carried by the push, the base's normal on open
//! shells.

use crate::common;
use crate::common::FakeProbe;
use xtrude_app::tools::pushpull::InferenceKind;
use xtrude_app::value_box::ArrayKind;

use common::{draw_rect, stub, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::tools::pushpull::{last_distance, set_last_distance, PushPullTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_app::viewport::{FaceHit, PickedEdge, Viewport};
use xtrude_core::math::VecExt;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::topology::{is_planar, tkey, PLANAR_TOLERANCE};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::MoveVerticesCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::pushpull::surface_normal;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn face_all_z(vp: &Viewport, f: FaceId, z: f64, tol: f64) -> bool {
    vp.scene
        .mesh()
        .face_positions(f)
        .iter()
        .all(|p| (p.z - z).abs() < tol)
}

fn cube(vp: &mut Viewport, size: f64, height: f64) {
    draw_rect(vp, &[v(0., 0.), v(size, 0.), v(size, size), v(0., size)]);
    let f = vp.scene.mesh().face_ids()[0];
    push(vp, f, height, false);
}

/// The tool set up on `face`, then committed.
fn push(vp: &mut Viewport, face: FaceId, dist: f64, keep: bool) {
    let mut t = setup(vp, face);
    t.pp.limit_in = None;
    t.pp.extrusion = dist;
    t.pp.keep_base = keep;
    t.commit(vp);
}

/// The tool locked on `face`: anchor, normal, classification, cap ring
/// and inward limit.
fn setup(vp: &Viewport, face: FaceId) -> PushPullTool {
    let mut t = PushPullTool::new();
    let mid = vp.scene.active_mesh_id();
    t.pp.setup_plain(vp.scene.mesh(), mid, face);
    t
}

/// Set up, extrusion set, clamped (no viewport).
fn locked_tool(vp: &Viewport, face: FaceId, dist: f64) -> PushPullTool {
    let mut t = setup(vp, face);
    t.pp.extrusion = dist;
    t.pp.clamp_extrusion();
    t
}

fn top(vp: &Viewport, z: f64) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face(f).outer.len() == 4 && face_all_z(vp, f, z, 1e-9))
        .expect("a 4-corner face at that height")
}

fn nfaces(vp: &Viewport) -> usize {
    vp.scene.mesh().faces.len()
}

fn ctx_at(x: f64, y: f64) -> ToolContext {
    let mut c = ToolContext::at(DVec3::ZERO);
    c.screen = (x, y);
    c
}

fn pick(vp: &mut Viewport, f: FaceId) {
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: vp.scene.active_mesh_id(),
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
}

fn add_rect_face(vp: &mut Viewport, ring: &[DVec3]) {
    draw_rect(vp, ring);
}

fn min_max(vp: &Viewport, f: FaceId, axis: usize) -> (f64, f64) {
    let ps = vp.scene.mesh().face_positions(f);
    let vals: Vec<f64> = ps.iter().map(|p| p[axis]).collect();
    (
        vals.iter().cloned().fold(f64::MAX, f64::min),
        vals.iter().cloned().fold(f64::MIN, f64::max),
    )
}

// ---- Ctrl: push/pull a copy --------------------------------------------------

#[test]
fn test_ctrl_push_keeps_base_as_dividing_face() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    let before = vp.history.undo_len();
    push(&mut vp, t3, 2.0, true);
    assert!(vp.scene.mesh().has_face(t3));
    top(&vp, 5.0);
    assert_eq!(nfaces(&vp), 11);
    let m = vp.scene.mesh();
    let belt: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.z - 3.0).abs() < 1e-9 && (b.z - 3.0).abs() < 1e-9
        })
        .collect();
    assert!(!belt.is_empty() && belt.iter().all(|&e| m.edge(e).faces.len() == 3));
    assert_eq!(vp.history.undo_len(), before + 1);
    assert!(vp.undo());
    assert_eq!(nfaces(&vp), 6);
}

#[test]
fn test_ctrl_push_overrides_prism_translation() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t = top(&vp, 3.0);
    push(&mut vp, t, 2.0, false);
    assert_eq!(nfaces(&vp), 6);
    let mut vp2 = stub();
    cube(&mut vp2, 4.0, 3.0);
    let t = top(&vp2, 3.0);
    push(&mut vp2, t, 2.0, true);
    assert_eq!(nfaces(&vp2), 11);
}

// ---- double-click repeats the last distance -----------------------------------

fn block_on_top(vp: &Viewport, pred: impl Fn(f64, f64) -> bool) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| {
            let (xmin, xmax) = min_max(vp, f, 0);
            m.face(f).outer.len() == 4 && face_all_z(vp, f, 3.0, 1e-9) && pred(xmin, xmax)
        })
        .expect("the block")
}

#[test]
fn test_double_click_repeats_last_distance() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    set_last_distance(None);
    add_rect_face(
        &mut vp,
        &[
            v3(1., 1., 3.),
            v3(2., 1., 3.),
            v3(2., 2., 3.),
            v3(1., 2., 3.),
        ],
    );
    let block = block_on_top(&vp, |_lo, hi| hi <= 2.001);
    push(&mut vp, block, 1.5, false);
    assert_eq!(last_distance(), Some(1.5));
    add_rect_face(
        &mut vp,
        &[
            v3(2.5, 2.5, 3.),
            v3(3.5, 2.5, 3.),
            v3(3.5, 3.5, 3.),
            v3(2.5, 3.5, 3.),
        ],
    );
    let block2 = block_on_top(&vp, |lo, _hi| lo >= 2.499);
    pick(&mut vp, block2);
    let mut t = PushPullTool::new();
    t.on_double_click(&mut vp, &ctx_at(0.0, 0.0));
    let m = vp.scene.mesh();
    let tops = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face(f).outer.len() == 4 && face_all_z(&vp, f, 4.5, 1e-9))
        .count();
    assert_eq!(tops, 2);
}

#[test]
fn test_double_click_without_history_is_plain_click() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    set_last_distance(None);
    let t3 = top(&vp, 3.0);
    pick(&mut vp, t3);
    let mut t = PushPullTool::new();
    t.hovered_face = Some((MeshId::LOOSE, t3));
    t.on_double_click(&mut vp, &ctx_at(0.0, 0.0));
    assert!(t.dragging());
    assert_eq!(nfaces(&vp), 6);
}

// ---- value box: negative reverses the direction -------------------------------

#[test]
fn test_value_box_negative_value_reverses_direction() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    add_rect_face(
        &mut vp,
        &[
            v3(1., 1., 3.),
            v3(2., 1., 3.),
            v3(2., 2., 3.),
            v3(1., 2., 3.),
        ],
    );
    let block = block_on_top(&vp, |_lo, hi| hi <= 2.001);
    let mut t = setup(&vp, block);
    t.pp.limit_in = None;
    t.pp.extrusion = 0.4;
    assert!(t.on_value(&mut vp, &ToolValue::Num(-1.0)));
    let m = vp.scene.mesh();
    assert!(m
        .face_ids()
        .into_iter()
        .any(|f| m.face(f).outer.len() == 4 && face_all_z(&vp, f, 2.0, 1e-9)));
    assert!(signed_volume(m) > 0.0);
}

#[test]
fn test_value_box_zero_rejected() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    let mut t = setup(&vp, t3);
    assert!(!t.on_value(&mut vp, &ToolValue::Num(0.0)));
}

// ---- value box parser: units + sign --------------------------------------------

#[test]
fn test_parse_value_buffer_units_and_sign() {
    let _m = MetreDocument::new();
    let p = |s: &str| parse_value_buffer(s, false);
    assert_eq!(p("2"), Some(Typed::Value(2.0)));
    assert_eq!(p("-2"), Some(Typed::Value(-2.0)));
    assert_eq!(p("30cm"), Some(Typed::Value(0.3)));
    assert_eq!(p("1500mm"), Some(Typed::Value(1.5)));
    assert_eq!(p("2m"), Some(Typed::Value(2.0)));
    assert_eq!(p("2,5"), Some(Typed::Value(2.5)));
    assert_eq!(p("1;2;50cm"), Some(Typed::Triple(1.0, 2.0, 0.5)));
    assert_eq!(p("-30cm"), Some(Typed::Value(-0.3)));
    assert_eq!(p("abc"), None);
    assert_eq!(
        p("2x"),
        Some(Typed::Array {
            count: 2,
            kind: ArrayKind::External,
            step: None
        })
    );
    assert_eq!(p("2xy"), None);
}

// ---- clamp ---------------------------------------------------------------------

#[test]
fn test_inward_limit_detected_on_prism_cap() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t = locked_tool(&vp, top(&vp, 3.0), -1.0);
    assert!(t.pp.limit_in.is_some_and(|l| (l - 3.0).abs() < 1e-6));
}

#[test]
fn test_shrink_beyond_height_clamps() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t = locked_tool(&vp, top(&vp, 3.0), -99.0);
    assert_eq!(t.pp.extrusion, -3.0);
}

#[test]
fn test_shrink_to_exact_limit_collapses_to_single_face() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let mut t = locked_tool(&vp, top(&vp, 3.0), -99.0);
    t.commit(&mut vp);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1);
    let f = m.face_ids()[0];
    assert!(face_all_z(&vp, f, 0.0, 1e-9));
    assert_eq!(m.edges.len(), 4);
    assert_eq!(m.verts.len(), 4);
}

#[test]
fn test_corner_step_clamped_opens_notch_through_floor() {
    let mut vp = stub();
    cube(&mut vp, 10.0, 3.0);
    add_rect_face(
        &mut vp,
        &[
            v3(0., 0., 3.),
            v3(4., 0., 3.),
            v3(4., 4., 3.),
            v3(0., 4., 3.),
        ],
    );
    let m = vp.scene.mesh();
    let corner = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            m.face(f).outer.len() == 4
                && face_all_z(&vp, f, 3.0, 1e-9)
                && min_max(&vp, f, 0).1 <= 4.001
                && min_max(&vp, f, 1).1 <= 4.001
        })
        .unwrap();
    let mut t = locked_tool(&vp, corner, -99.0);
    assert_eq!(t.pp.extrusion, -3.0);
    t.commit(&mut vp);
    let m = vp.scene.mesh();
    assert!(
        m.edge_ids().into_iter().all(|e| m.edge(e).faces.len() == 2),
        "not watertight"
    );
    assert!(signed_volume(m) > 0.0);
    let floors: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| face_all_z(&vp, f, 0.0, 1e-9))
        .collect();
    assert_eq!(floors.len(), 1);
    assert_eq!(m.face(floors[0]).outer.len(), 6);
}

#[test]
fn test_through_target_does_not_clamp() {
    let mut vp = stub();
    add_rect_face(
        &mut vp,
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 0.3, 0.),
            v3(0., 0.3, 0.),
        ],
    );
    let f = vp.scene.mesh().face_ids()[0];
    push(&mut vp, f, 3.0, false);
    add_rect_face(
        &mut vp,
        &[
            v3(1., 0., 1.),
            v3(3., 0., 1.),
            v3(3., 0., 2.),
            v3(1., 0., 2.),
        ],
    );
    let m = vp.scene.mesh();
    let win = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            let ps = m.face_positions(f);
            ps.len() == 4
                && ps.iter().all(|p| p.y.abs() < 1e-9)
                && min_max(&vp, f, 0).1 <= 3.001
                && min_max(&vp, f, 0).0 >= 0.999
        })
        .unwrap();
    let mut t = locked_tool(&vp, win, -0.4);
    assert!(t.pp.limit_in.is_none());
    assert_eq!(t.pp.extrusion, -0.4);
    t.commit(&mut vp);
    let m = vp.scene.mesh();
    let backs = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            m.face_positions(f).iter().all(|p| (p.y - 0.3).abs() < 1e-4)
                && !m.face(f).holes.is_empty()
        })
        .count();
    assert_eq!(backs, 1);
}

#[test]
fn test_outward_pull_never_clamped() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t = locked_tool(&vp, top(&vp, 3.0), 50.0);
    assert_eq!(t.pp.extrusion, 50.0);
}

// ---- distance inference --------------------------------------------------------

fn top_view(vp: &mut Viewport) {
    FakeProbe::of(vp).world_to_pixel(|w: DVec3| Some((w.x * 10.0, -w.z * 10.0)));
}

/// Cube A (top z=3) and a block at x=6..8 pushed to z=5.
fn cube_and_block() -> Viewport {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    add_rect_face(&mut vp, &[v(6., 0.), v(8., 0.), v(8., 2.), v(6., 2.)]);
    let m = vp.scene.mesh();
    let r = m
        .face_ids()
        .into_iter()
        .find(|&f| face_all_z(&vp, f, 0.0, 1e-9) && min_max(&vp, f, 0).0 >= 5.999)
        .unwrap();
    let d = if m.face_normal(r).z > 0.0 { 5.0 } else { -5.0 };
    push(&mut vp, r, d, false);
    vp
}

#[test]
fn test_hovering_vertex_infers_distance() {
    let mut vp = cube_and_block();
    top_view(&mut vp);
    let mut t = setup(&vp, top(&vp, 3.0));
    t.pp.limit_in = None;
    let d = t.infer_reference_distance(&mut vp, &ctx_at(60.0, -50.0));
    assert!(d.is_some_and(|d| (d - 2.0).abs() < 1e-6), "{d:?}");
    assert!(t
        .infer_reference_distance(&mut vp, &ctx_at(500.0, 500.0))
        .is_none());
    assert!(t
        .infer_reference_distance(&mut vp, &ctx_at(0.0, -30.0))
        .is_none());
}

#[test]
fn test_hovering_face_infers_distance_and_marks_it() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    add_rect_face(
        &mut vp,
        &[
            v3(20., 20., 7.),
            v3(24., 20., 7.),
            v3(24., 24., 7.),
            v3(20., 24., 7.),
        ],
    );
    let m = vp.scene.mesh();
    let r = m
        .face_ids()
        .into_iter()
        .find(|&f| face_all_z(&vp, f, 7.0, 1e-9))
        .unwrap();
    let mut t = setup(&vp, top(&vp, 3.0));
    t.pp.limit_in = None;
    top_view(&mut vp);
    pick(&mut vp, r);
    FakeProbe::of(&mut vp).pixel_to_ray(|x: f64, _y: f64| {
        Some((DVec3::new(x / 10.0, 1.0, 100.0), DVec3::new(0.0, 0.0, -1.0)))
    });
    let d = t.infer_reference_distance(&mut vp, &ctx_at(220.0, -70.0));
    assert!(d.is_some_and(|d| (d - 4.0).abs() < 1e-6), "{d:?}");
    let (pt, kind) = t.inference_marker().unwrap();
    assert_eq!(kind, InferenceKind::Face);
    assert!((pt.z - 7.0).abs() < 1e-6);
}

#[test]
fn test_clamp_flashes_status_message() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let mut t = setup(&vp, top(&vp, 3.0));
    t.pp.extrusion = -99.0;
    t.clamp(&mut vp);
    assert_eq!(t.pp.extrusion, -3.0);
    assert_eq!(
        vp.outbox.status_flash.as_ref().map(|s| s.0.as_str()),
        Some("Push limited to 3.00 m — deeper would leave the solid")
    );
}

// ---- refusal guard ---------------------------------------------------------------

#[test]
fn test_push_that_would_break_the_solid_is_refused() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let mut t = setup(&vp, top(&vp, 3.0));
    t.pp.extrusion = 1.0;
    let before_faces = nfaces(&vp);
    let before_vol = signed_volume(vp.scene.mesh());
    fn break_it(m: &mut Mesh) {
        let f = m.face_ids()[0];
        m.remove_face(f);
    }
    t.pp.inner_override = Some(break_it);
    t.pp.mutate(&mut vp.scene);
    assert!(t.pp.refused);
    assert!(is_closed(vp.scene.mesh()));
    assert_eq!(nfaces(&vp), before_faces);
    assert!((signed_volume(vp.scene.mesh()) - before_vol).abs() < 1e-9);
}

#[test]
fn test_valid_push_is_not_refused() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let mut t = locked_tool(&vp, top(&vp, 3.0), 2.0);
    t.commit(&mut vp);
    assert!(is_closed(vp.scene.mesh()));
    assert!((signed_volume(vp.scene.mesh()) - 80.0).abs() < 1e-6);
    assert_eq!(last_distance(), Some(2.0));
}

// ---- autofold ----------------------------------------------------------------------

#[test]
fn test_move_lifting_a_corner_autofolds_quad() {
    let mut vp = stub();
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.run(MoveVerticesCommand {
        src: vec![v(2., 2.)],
        delta: v3(0., 0., 1.),
    });
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    assert!(m.face_ids().into_iter().all(|f| m.face(f).outer.len() == 3));
    assert!(m
        .face_ids()
        .into_iter()
        .all(|f| is_planar(&m.face_positions(f), PLANAR_TOLERANCE)));
    assert_eq!(
        m.edge_ids()
            .into_iter()
            .filter(|&e| m.edge(e).faces.len() == 2)
            .count(),
        1
    );
    assert!(vp.undo());
    let m = vp.scene.mesh();
    assert!(m.faces.len() == 1 && m.face(m.face_ids()[0]).outer.len() == 4);
    assert!(vp.redo());
    assert_eq!(vp.scene.mesh().faces.len(), 2);
}

#[test]
fn test_move_in_plane_does_not_fold() {
    let mut vp = stub();
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.run(MoveVerticesCommand {
        src: vec![v(2., 2.)],
        delta: v3(1., 1., 0.),
    });
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1);
    assert_eq!(m.face(m.face_ids()[0]).outer.len(), 4);
    assert!(vp.undo());
    let m = vp.scene.mesh();
    assert!((m.face_positions(m.face_ids()[0])[2].x - 2.0).abs() < 1e-6);
}

#[test]
fn test_autofold_pentagon_folds_minimally() {
    let mut vp = stub();
    vp.scene.mesh_mut().add_face(
        &[v(0., 0.), v(4., 0.), v(4., 4.), v(2., 6.), v(0., 4.)],
        &[],
    );
    vp.run(MoveVerticesCommand {
        src: vec![v(2., 6.)],
        delta: v3(0., 0., 1.5),
    });
    let m = vp.scene.mesh();
    assert!(m
        .face_ids()
        .into_iter()
        .all(|f| is_planar(&m.face_positions(f), PLANAR_TOLERANCE)));
    assert!(m.faces.len() <= 3);
}

// ---- push/pull on a group's face -----------------------------------------------------

fn boxed_group(vp: &mut Viewport, x0: f64) -> GroupId {
    let mut m = Mesh::new();
    let p = [
        v3(x0, 0., 0.),
        v3(x0 + 2., 0., 0.),
        v3(x0 + 2., 2., 0.),
        v3(x0, 2., 0.),
    ];
    let q = [
        v3(x0, 0., 2.),
        v3(x0 + 2., 0., 2.),
        v3(x0 + 2., 2., 2.),
        v3(x0, 2., 2.),
    ];
    m.add_face(&p, &[]);
    m.add_face(&q, &[]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        m.add_face(&[p[i], p[j], q[j], q[i]], &[]);
    }
    orient_outward(&mut m, DEFAULT_SEED, None);
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(id, None);
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    gid
}

fn group_mesh(vp: &Viewport, g: GroupId) -> MeshId {
    vp.scene.doc.find_group(g).unwrap().mesh
}

fn group_top(vp: &Viewport, gm: MeshId, z: f64) -> FaceId {
    let m = vp.scene.doc.mesh(gm);
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - z).abs() < 1e-9))
        .unwrap()
}

#[test]
fn test_push_on_group_face_edits_only_the_group() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let g = boxed_group(&mut vp, 10.0);
    let gm = group_mesh(&vp, g);
    let loose = nfaces(&vp);
    let f = group_top(&vp, gm, 2.0);
    let mut t = PushPullTool::new();
    t.pp.setup_plain(vp.scene.doc.mesh(gm), gm, f);
    t.pp.extrusion = 1.0;
    t.commit(&mut vp);
    assert_eq!(nfaces(&vp), loose);
    let m = vp.scene.doc.mesh(gm);
    assert!(m
        .face_ids()
        .into_iter()
        .any(|f| m.face_positions(f).iter().all(|p| (p.z - 3.0).abs() < 1e-9)));
    assert!(m.edge_ids().into_iter().all(|e| m.edge(e).faces.len() == 2));
    assert!(signed_volume(m) > 0.0);
    assert!(vp.undo());
    let gm = group_mesh(&vp, g);
    let m = vp.scene.doc.mesh(gm);
    assert!(m
        .face_ids()
        .into_iter()
        .any(|f| m.face_positions(f).iter().all(|p| (p.z - 2.0).abs() < 1e-9)));
    assert!(!m.verts.iter().any(|(_, v)| v.position.z > 2.5));
}

#[test]
fn test_group_recess_and_clamp_use_group_geometry() {
    let mut vp = stub();
    let g = boxed_group(&mut vp, 0.0);
    let gm = group_mesh(&vp, g);
    let f = group_top(&vp, gm, 2.0);
    let mut t = PushPullTool::new();
    t.pp.setup_plain(vp.scene.doc.mesh(gm), gm, f);
    assert!(t.pp.limit_in.is_some_and(|l| (l - 2.0).abs() < 1e-6));
    t.pp.extrusion = -99.0;
    t.pp.clamp_extrusion();
    assert_eq!(t.pp.extrusion, -2.0);
    t.commit(&mut vp);
    assert_eq!(vp.scene.doc.mesh(group_mesh(&vp, g)).faces.len(), 1);
    assert!(vp.scene.mesh().faces.is_empty());
}

// ---- floor-plan workflow ----------------------------------------------------------------

#[test]
fn test_two_cell_outline_raises_cleanly() {
    let mut vp = stub();
    draw_rect(&mut vp, &[v(0., 0.), v(3., 0.), v(3., 4.), v(0., 4.)]);
    draw_rect(&mut vp, &[v(3., 0.), v(6., 0.), v(6., 4.), v(3., 4.)]);
    let m = vp.scene.mesh();
    let room_a = m
        .face_ids()
        .into_iter()
        .min_by(|a, b| m.face_centroid(*a).x.total_cmp(&m.face_centroid(*b).x))
        .unwrap();
    push(&mut vp, room_a, 2.7, false);
    let m = vp.scene.mesh();
    let room_b = m
        .face_ids()
        .into_iter()
        .find(|&f| face_all_z(&vp, f, 0.0, 1e-9) && m.face_centroid(f).x > 3.0)
        .unwrap();
    push(&mut vp, room_b, 2.7, false);
    let m = vp.scene.mesh();
    let tops = m
        .face_ids()
        .into_iter()
        .filter(|&f| face_all_z(&vp, f, 2.7, 1e-6))
        .count();
    let shared = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.x - 3.0).abs() < 1e-6))
        .count();
    assert_eq!(tops, 2);
    assert_eq!(shared, 1);
    let ridge: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.z - 2.7).abs() < 1e-6
                && (b.z - 2.7).abs() < 1e-6
                && (a.x - 3.0).abs() < 1e-6
                && (b.x - 3.0).abs() < 1e-6
        })
        .collect();
    assert!(ridge.len() == 1 && m.edge(ridge[0]).faces.len() == 3);
    assert_eq!(
        m.edge_ids()
            .into_iter()
            .filter(|&e| m.edge(e).faces.is_empty())
            .count(),
        0
    );
    let seams = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let fs = &m.edge(e).faces;
            fs.len() == 2
                && m.face_normal(fs[0])
                    .unit_or_zero()
                    .dot(m.face_normal(fs[1]).unit_or_zero())
                    > 0.999
        })
        .count();
    assert_eq!(seams, 0);
}

// ---- drag preview -----------------------------------------------------------------------

fn preview_tool(vp: &mut Viewport, face: FaceId, normal: DVec3, dist: f64) -> PushPullTool {
    let mut t = PushPullTool::new();
    t.pp.base_face = Some(face);
    t.pp.target = Some(vp.scene.active_mesh_id());
    t.pp.normal = Some(normal);
    t.pp.extrusion = dist;
    t.show_light_preview(vp);
    t
}

type Seg = ((i64, i64, i64), (i64, i64, i64));

fn key(p: DVec3) -> (i64, i64, i64) {
    (
        (p.x * 1e6).round() as i64,
        (p.y * 1e6).round() as i64,
        (p.z * 1e6).round() as i64,
    )
}

fn seg_set(segs: &[(DVec3, DVec3)]) -> std::collections::BTreeSet<Seg> {
    segs.iter()
        .map(|(a, b)| {
            let (ka, kb) = (key(*a), key(*b));
            if ka <= kb {
                (ka, kb)
            } else {
                (kb, ka)
            }
        })
        .collect()
}

#[test]
fn test_drag_preview_draws_the_sweep_wireframe() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    let n = vp.scene.mesh().face_normal(t3);
    let mut t = preview_tool(&mut vp, t3, n, 2.0);
    let segs = t.rubber_band_lines(&vp);
    assert!(!segs.is_empty(), "the drag preview drew no edges at all");
    let drawn = seg_set(&segs);
    assert_eq!(drawn.len(), 8);
    let zs: std::collections::BTreeSet<i64> = drawn.iter().flat_map(|(a, b)| [a.2, b.2]).collect();
    assert_eq!(zs, [3_000_000i64, 5_000_000].into_iter().collect());
    assert_eq!(drawn.iter().filter(|(a, b)| a.2 != b.2).count(), 4);
    t.commit(&mut vp);
    assert!(t.rubber_band_lines(&vp).is_empty());
}

#[test]
fn test_drag_preview_wireframe_carries_holes() {
    let mut vp = stub();
    let ring = vp.scene.mesh_mut().add_face(
        &[v(0., 0.), v(6., 0.), v(6., 6.), v(0., 6.)],
        &[vec![v(2., 2.), v(4., 2.), v(4., 4.), v(2., 4.)]],
    );
    assert!(!vp.scene.mesh().face(ring).holes.is_empty());
    let n = vp.scene.mesh().face_normal(ring);
    let t = preview_tool(&mut vp, ring, n, 1.0);
    let segs = t.rubber_band_lines(&vp);
    assert_eq!(segs.len(), 16);
    let xs: Vec<i64> = segs
        .iter()
        .map(|(a, _)| (a.x * 1e6).round() as i64)
        .collect();
    assert!(xs.contains(&2_000_000) && xs.contains(&4_000_000));
}

// ---- the material extrudes with the shape -------------------------------------------------

fn painted_rect(vp: &mut Viewport, paint: xtrude_core::attrs::Paint) -> FaceId {
    draw_rect(vp, &[v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)]);
    let f = vp.scene.mesh().face_ids()[0];
    vp.scene.mesh_mut().face_mut(f).attrs.paint = paint;
    f
}

fn paint(c: [f64; 3], mat: &str) -> xtrude_core::attrs::Paint {
    xtrude_core::attrs::Paint {
        color: Some(c),
        mat: Some(mat.into()),
        opacity: None,
    }
}

#[test]
fn test_push_carries_the_colour_onto_the_new_sides() {
    let mut vp = stub();
    let p = paint([0.8, 0.2, 0.1], "Brick red");
    let r = painted_rect(&mut vp, p.clone());
    push(&mut vp, r, 3.0, false);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 6);
    assert!(m.face_ids().into_iter().all(|f| m.face(f).attrs.paint == p));
}

#[test]
fn test_push_carries_the_paint_onto_the_walls_and_the_cap() {
    let mut vp = stub();
    let p = paint([0.6, 0.5, 0.4], "Stone");
    let r = painted_rect(&mut vp, p.clone());
    push(&mut vp, r, 3.0, false);
    let m = vp.scene.mesh();
    let walls: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_normal(f).unit_or_zero().z.abs() < 1e-6)
        .collect();
    assert_eq!(walls.len(), 4);
    for w in walls {
        assert_eq!(m.face(w).attrs.paint, p, "the wall came out bare");
    }
    let cap = top(&vp, 3.0);
    assert_eq!(
        vp.scene.mesh().face(cap).attrs.paint.color,
        Some([0.6, 0.5, 0.4])
    );
}

#[test]
fn test_push_leaves_a_face_that_has_its_own_paint_alone() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    for f in vp.scene.mesh().face_ids() {
        let mat = if f == t3 { "New" } else { "Original" };
        vp.scene.mesh_mut().face_mut(f).attrs.paint.mat = Some(mat.into());
    }
    push(&mut vp, t3, 2.0, false);
    let m = vp.scene.mesh();
    assert!(
        m.face_ids()
            .into_iter()
            .any(|f| m.face(f).attrs.paint.mat.as_deref() == Some("Original")),
        "the untouched faces lost their own material"
    );
}

#[test]
fn test_unpainted_push_stays_unpainted() {
    let mut vp = stub();
    let r = painted_rect(&mut vp, Default::default());
    push(&mut vp, r, 3.0, false);
    let m = vp.scene.mesh();
    assert!(m.face_ids().into_iter().all(|f| m.face(f).attrs.is_empty()));
}

// ---- the drag wireframe follows the commit's softening -------------------------------------

fn ngon_face(vp: &mut Viewport, n: usize, r: f64) -> FaceId {
    let ring: Vec<DVec3> = (0..n)
        .map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            v(r * a.cos(), r * a.sin())
        })
        .collect();
    vp.scene.mesh_mut().add_face(&ring, &[])
}

#[test]
fn test_pushing_a_circle_previews_a_smooth_cylinder() {
    let mut vp = stub();
    let c = ngon_face(&mut vp, 24, 2.0);
    let n = vp.scene.mesh().face_normal(c);
    let t = preview_tool(&mut vp, c, n, 3.0);
    let segs = t.rubber_band_lines(&vp);
    let risers = segs
        .iter()
        .filter(|(a, b)| (key(*a).2 - key(*b).2).abs() > 1)
        .count();
    assert_eq!(risers, 0, "the cylinder's side is drawn with facet lines");
    assert_eq!(segs.len(), 24);
}

#[test]
fn test_pushing_a_hexagon_keeps_its_real_edges() {
    let mut vp = stub();
    let h = ngon_face(&mut vp, 6, 2.0);
    let n = vp.scene.mesh().face_normal(h);
    let t = preview_tool(&mut vp, h, n, 3.0);
    let segs = t.rubber_band_lines(&vp);
    assert_eq!(
        segs.iter()
            .filter(|(a, b)| (a.z - b.z).abs() > 1e-6)
            .count(),
        6
    );
    assert_eq!(segs.len(), 12);
}

// ---- the base's normal agrees with its surface --------------------------------------------

fn panel_with_notch(vp: &mut Viewport) -> (FaceId, FaceId) {
    let notch_ring = [
        v3(1., 0., 0.),
        v3(1., 0., 2.),
        v3(2.5, 0., 2.),
        v3(2.5, 0., 0.),
    ];
    let outer = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 0., 2.),
        v3(2.5, 0., 2.),
        v3(2.5, 0., 0.),
        v3(4., 0., 0.),
        v3(4., 0., 3.),
        v3(0., 0., 3.),
    ];
    let m = vp.scene.mesh_mut();
    let mut wall = m.add_face(&outer, &[]);
    if m.face_normal(wall).unit_or_zero().y > 0.0 {
        m.remove_face(wall);
        let rev: Vec<DVec3> = outer.iter().rev().copied().collect();
        wall = m.add_face(&rev, &[]);
    }
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(2.5, 0., 0.),
            v3(4., 0., 0.),
            v3(4., 3., 0.),
            v3(0., 3., 0.),
        ],
        &[],
    );
    let mut notch = m.add_face(&notch_ring, &[]);
    if m.face_normal(notch).unit_or_zero().y < 0.0 {
        m.remove_face(notch);
        let rev: Vec<DVec3> = notch_ring.iter().rev().copied().collect();
        notch = m.add_face(&rev, &[]);
    }
    (wall, notch)
}

#[test]
fn test_a_reversed_face_takes_the_facing_of_its_own_surface() {
    let mut vp = stub();
    let (wall, notch) = panel_with_notch(&mut vp);
    let m = vp.scene.mesh();
    assert!(m.face_normal(notch).unit_or_zero().y > 0.0);
    assert!(m.face_normal(wall).unit_or_zero().y < 0.0);
    assert!(
        surface_normal(m, notch).unit_or_zero().y < 0.0,
        "the notch still disagrees with its wall"
    );
}

#[test]
fn test_a_lone_sheet_keeps_its_own_normal() {
    let mut vp = stub();
    let sheet = vp
        .scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    let m = vp.scene.mesh();
    let n0 = m.face_normal(sheet).unit_or_zero();
    let n = surface_normal(m, sheet).unit_or_zero();
    assert!((n.dot(n0) - 1.0).abs() < 1e-9);
}

#[test]
fn test_pushing_a_reversed_notch_inward_hides_the_base_face() {
    let mut vp = stub();
    let (_wall, notch) = panel_with_notch(&mut vp);
    let drag = |vp: &mut Viewport, normal: DVec3| {
        let mut t = PushPullTool::new();
        t.pp.base_face = Some(notch);
        t.pp.target = Some(MeshId::LOOSE);
        t.pp.normal = Some(normal);
        let (a, p) = t.pp.classify_base(vp.scene.mesh());
        t.pp.attached = a;
        t.pp.prism_cap = p;
        t.pp.extrusion = if normal.unit_or_zero().y < 0.0 {
            -0.6
        } else {
            0.6
        };
        t.show_light_preview(vp);
        (t.pp.extrusion, vp.suppressed_faces().clone())
    };
    let n_own = vp.scene.mesh().face_normal(notch);
    let (dist, hidden) = drag(&mut vp, n_own);
    assert!(dist > 0.0 && hidden.is_empty());
    let n_surf = surface_normal(vp.scene.mesh(), notch);
    let (dist, hidden) = drag(&mut vp, n_surf);
    assert!(dist < 0.0, "a drag into the wall must read as negative");
    assert_eq!(
        hidden,
        [(MeshId::LOOSE, notch)].into_iter().collect(),
        "the base face was left covering the recess"
    );
}

#[test]
fn test_a_reversed_notch_commits_the_push_the_way_it_previewed() {
    let mut vp = stub();
    let (_wall, notch) = panel_with_notch(&mut vp);
    let inside = 1.0;
    assert!(vp.scene.mesh().face_normal(notch).unit_or_zero().y > 0.0);
    let mut t = PushPullTool::new();
    {
        let m = vp.scene.mesh();
        t.pp.base_face = Some(notch);
        t.pp.target = Some(MeshId::LOOSE);
        t.pp.normal = Some(surface_normal(m, notch));
        t.pp.anchor = Some(m.face_centroid(notch));
        t.pp.drag_pre_oriented = true;
        let (a, p) = t.pp.classify_base(m);
        t.pp.attached = a;
        t.pp.prism_cap = p;
        t.pp.cap_positions = xtrude_model::pushpull::cap_loop_positions(m, notch);
    }
    t.pp.extrusion = -0.6;
    t.commit(&mut vp);
    let m = vp.scene.mesh();
    let moved: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            m.face_normal(f).unit_or_zero().y.abs() > 0.99
                && (m.face_area(f) - 3.0).abs() < 1e-6
                && m.face_centroid(f).y.abs() > 1e-6
        })
        .collect();
    assert!(!moved.is_empty(), "the notch did not move at all");
    let y = m.face_centroid(moved[0]).y;
    assert!(
        y * inside > 0.0,
        "the push came out the wrong way: the pocket floor landed at y={y:.3}"
    );
}

// ---- a prism extend/shrink previews by moving the model -------------------------------------

fn fingerprint(vp: &Viewport) -> Vec<(usize, i64, i64, i64, i64)> {
    let m = vp.scene.mesh();
    let r = |x: f64| (x * 1e6).round() as i64;
    let mut out: Vec<_> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let c = m.face_centroid(f);
            (
                m.face(f).outer.len(),
                r(m.face_area(f)),
                r(c.x),
                r(c.y),
                r(c.z),
            )
        })
        .collect();
    out.sort();
    out
}

fn prism_drag(vp: &mut Viewport, face: FaceId, dists: &[f64]) -> PushPullTool {
    let mut t = PushPullTool::new();
    {
        let m = vp.scene.mesh();
        t.pp.base_face = Some(face);
        t.pp.target = Some(MeshId::LOOSE);
        t.pp.normal = Some(m.face_normal(face));
        t.pp.anchor = Some(m.face_centroid(face));
        let (a, p) = t.pp.classify_base(m);
        t.pp.attached = a;
        t.pp.prism_cap = p;
        t.pp.cap_positions = xtrude_model::pushpull::cap_loop_positions(m, face);
        let keys: std::collections::HashSet<_> =
            t.pp.cap_positions.iter().map(|&p| tkey(p)).collect();
        t.pp.prism_verts = m
            .vertex_ids()
            .into_iter()
            .filter(|&v| keys.contains(&tkey(m.pos(v))))
            .collect();
    }
    assert!(t.pp.prism_cap, "this is meant to be the prism case");
    for &d in dists {
        t.pp.extrusion = d;
        t.show_prism_preview(vp);
    }
    t
}

#[test]
fn test_the_prism_preview_moves_the_model_and_adds_no_overlay() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    let t = prism_drag(&mut vp, t3, &[1.0]);
    assert!(t.preview_faces().is_empty());
    assert!(t.rubber_band_lines(&vp).is_empty());
    top(&vp, 4.0);
    assert_eq!(nfaces(&vp), 6);
}

#[test]
fn test_the_prism_preview_moves_only_the_delta_and_reverts_exactly() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let t3 = top(&vp, 3.0);
    let before = fingerprint(&vp);
    let mut t = prism_drag(&mut vp, t3, &[0.5, 1.7, 1.2, -0.8, 2.0]);
    top(&vp, 5.0);
    assert_eq!(t.pp.prism_applied, 2.0);
    t.revert_preview(&mut vp);
    assert_eq!(t.pp.prism_applied, 0.0);
    assert_eq!(fingerprint(&vp), before, "the drag left the model moved");
}

#[test]
fn test_a_prism_drag_commits_the_same_as_a_direct_push() {
    let mut direct = stub();
    cube(&mut direct, 4.0, 3.0);
    let t = top(&direct, 3.0);
    push(&mut direct, t, 1.5, false);
    let mut dragged = stub();
    cube(&mut dragged, 4.0, 3.0);
    let t = top(&dragged, 3.0);
    let mut tool = prism_drag(&mut dragged, t, &[0.4, 1.1, 1.5]);
    tool.commit(&mut dragged);
    assert_eq!(fingerprint(&dragged), fingerprint(&direct));
}

#[test]
fn test_pushing_a_block_back_flush_does_not_drag_the_wall_with_it() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let notch = [
        v3(1., 0., 0.),
        v3(1., 0., 2.),
        v3(2.5, 0., 2.),
        v3(2.5, 0., 0.),
    ];
    let cmd = build_add_edges(
        &vp.scene,
        &[
            (notch[0], notch[1]),
            (notch[1], notch[2]),
            (notch[2], notch[3]),
        ],
        true,
        Vec::new(),
        None,
    );
    vp.execute_boxed(cmd);
    let m = vp.scene.mesh();
    let panel = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.y.abs() < 1e-9))
        .min_by(|a, b| m.face_area(*a).total_cmp(&m.face_area(*b)))
        .unwrap();
    push(&mut vp, panel, 2.0, false);
    let m = vp.scene.mesh();
    let cap = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.y + 2.0).abs() < 1e-6))
        .min_by(|a, b| m.face_area(*a).total_cmp(&m.face_area(*b)))
        .unwrap();
    let before = fingerprint(&vp);
    let all_y = |vp: &Viewport| {
        let m = vp.scene.mesh();
        let mut ys: Vec<i64> = m
            .face_ids()
            .into_iter()
            .flat_map(|f| m.face_positions(f))
            .map(|p| (p.y * 1e6).round() as i64)
            .collect();
        ys.sort();
        ys
    };
    let far = all_y(&vp);
    let mut t = prism_drag(&mut vp, cap, &[-0.5, -1.5, -1.95, -2.0, -2.2]);
    let m = vp.scene.mesh();
    assert!(m
        .face_ids()
        .into_iter()
        .flat_map(|f| m.face_positions(f))
        .all(|p| p.y >= -0.2 - 1e-6));
    t.revert_preview(&mut vp);
    assert_eq!(fingerprint(&vp), before, "the drag warped the model");
    assert_eq!(all_y(&vp), far);
}

#[test]
fn test_hovering_an_edge_infers_distance_and_says_on_edge() {
    let mut vp = cube_and_block();
    let m = vp.scene.mesh();
    let top_edge = m
        .edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.z - 5.0).abs() < 1e-9
                && (b.z - 5.0).abs() < 1e-9
                && a.y.abs() < 1e-9
                && b.y.abs() < 1e-9
        })
        .unwrap();
    let (a, b) = m.edge_ab(top_edge);
    let mut t = setup(&vp, top(&vp, 3.0));
    t.pp.limit_in = None;
    top_view(&mut vp);
    vp.inference.hover_edge = Some(PickedEdge {
        a,
        b,
        edge: Some((MeshId::LOOSE, top_edge)),
        in_group: false,
        context: None,
        group: None,
        soft: false,
        curve: None,
    });
    FakeProbe::of(&mut vp).project_to_lock_line(|start: DVec3, dir: DVec3, px: f64, py: f64| {
        let cursor = DVec3::new(px / 10.0, start.y, -py / 10.0);
        let d = dir.unit_or_zero();
        start + d * (cursor - start).dot(d)
    });
    let d = t.infer_reference_distance(&mut vp, &ctx_at(70.0, -50.0));
    assert!(d.is_some_and(|d| (d - 2.0).abs() < 1e-6), "{d:?}");
    let (pt, kind) = t.inference_marker().unwrap();
    assert_eq!(kind, InferenceKind::Edge);
    assert!((pt.z - 5.0).abs() < 1e-6 && (pt.x - 7.0).abs() < 1e-6);
}
