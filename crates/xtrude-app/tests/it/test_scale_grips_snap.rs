// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A held Scale grip snaps to another object's points, projected onto the
//! grip's line; faces and directional inferences do not pull it; what is being
//! scaled is left out of the snap.

use crate::common;
use crate::common::FakeProbe;
use xtrude_app::input::ScreenPt;

use common::{stub, v3, window};
use glam::DVec3;
use xtrude_app::input::MouseEvent;
use xtrude_app::tools::scale::{Grip, ScaleTool};
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

/// The bottom and top of a box
/// selected, the Scale tool activated on them.
fn tool_with_box(w: f64, d: f64, h: f64) -> (ScaleTool, Viewport) {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let f1 = m.add_face(
        &[v3(0., 0., 0.), v3(w, 0., 0.), v3(w, d, 0.), v3(0., d, 0.)],
        &[],
    );
    let f2 = m.add_face(
        &[v3(0., 0., h), v3(w, 0., h), v3(w, d, h), v3(0., d, h)],
        &[],
    );
    for f in [f1, f2] {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    (t, vp)
}

fn grip(t: &ScaleTool, want: (f64, f64, f64)) -> Grip {
    let r = |x: f64| (x * 1e6).round() / 1e6;
    t.grips
        .iter()
        .find(|g| {
            let p = t.grip_pos(g);
            (r(p.x), r(p.y), r(p.z)) == want
        })
        .cloned()
        .expect("a grip there")
}

fn ctx(kind: SnapKind, p: DVec3) -> ToolContext {
    let mut c = ToolContext::at(p);
    c.snap = Some(SnapResult::new(p, kind, [1., 1., 1.]));
    c
}

fn approx(f: Option<[f64; 3]>, want: [f64; 3]) {
    let f = f.expect("factors");
    for i in 0..3 {
        assert!((f[i] - want[i]).abs() < 1e-9, "{f:?} != {want:?}");
    }
}

#[test]
fn test_no_snap_until_a_grip_is_held() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    assert!(!t.snap_prefs().uses_snap && t.snap_excluded(&vp).is_none());
    let g = grip(&t, (4.0, 1.5, 1.0));
    t.grab(&mut vp, g, (0., 0.));
    assert!(t.snap_prefs().uses_snap);
    let ex = t.snap_excluded(&vp).unwrap();
    assert!(!ex.edges.is_empty() && ex.groups.is_empty());
}

#[test]
fn test_a_face_grip_lands_level_with_a_snapped_endpoint() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (4.0, 1.5, 1.0)); // the +X face grip
    t.grab(&mut vp, g, (0., 0.));
    let f = t.factors_for(&vp, &ctx(SnapKind::Endpoint, v3(7., 9., -3.)));
    approx(f, [7.0 / 4.0, 1.0, 1.0]); // only x, projected
}

#[test]
fn test_a_corner_grip_scales_uniformly_to_the_point_projected_on_its_diagonal() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (4.0, 3.0, 2.0)); // anchor (0,0,0)
    t.grab(&mut vp, g, (0., 0.));
    let f = t.factors_for(&vp, &ctx(SnapKind::Midpoint, v3(8., 6., 4.)));
    approx(f, [2.0, 2.0, 2.0]);
}

#[test]
fn test_an_edge_grip_takes_both_axes_from_the_point() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (4.0, 3.0, 1.0)); // x and y
    t.grab(&mut vp, g, (0., 0.));
    let f = t.factors_for(&vp, &ctx(SnapKind::Intersection, v3(6., 1.5, 9.)));
    approx(f, [1.5, 0.5, 1.0]);
}

#[test]
fn test_faces_and_directional_inferences_leave_the_grip_on_the_cursor() {
    // A fixed cursor ray gives the cursor's factors, which every such snap must
    // leave alone.
    for kind in [
        SnapKind::OnFace,
        SnapKind::Axis,
        SnapKind::Reference,
        SnapKind::None,
    ] {
        let (mut t, mut vp) = tool_with_box(4., 3., 2.);
        FakeProbe::of(&mut vp).pixel_to_ray(|_, _| Some((v3(6., -10., 1.), DVec3::Y)));
        let g = grip(&t, (4.0, 1.5, 1.0));
        t.grab(&mut vp, g, (0., 0.));
        let cursor = t.factors_for(&vp, &ToolContext::at(DVec3::ZERO));
        approx(cursor, [1.5, 1.0, 1.0]);
        assert_eq!(
            t.factors_for(&vp, &ctx(kind.clone(), v3(9., 9., 9.))),
            cursor,
            "{kind:?}"
        );
    }
}

fn add_box(m: &mut Mesh, lo: DVec3, hi: DVec3) -> Vec<FaceId> {
    let p = [
        v3(lo.x, lo.y, lo.z),
        v3(hi.x, lo.y, lo.z),
        v3(hi.x, hi.y, lo.z),
        v3(lo.x, hi.y, lo.z),
        v3(lo.x, lo.y, hi.z),
        v3(hi.x, lo.y, hi.z),
        v3(hi.x, hi.y, hi.z),
        v3(lo.x, hi.y, hi.z),
    ];
    [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ]
    .iter()
    .map(|idx| m.add_face(&idx.map(|i| p[i]), &[]))
    .collect()
}

#[test]
fn test_in_the_viewport_the_grip_lands_on_another_boxs_corner() {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1000., 700.);
    vp.camera.set_aspect(1000, 700);
    vp.scene.doc.guides.clear();
    let cube = add_box(vp.scene.mesh_mut(), v3(0., 0., 0.), v3(1., 1., 1.));
    add_box(vp.scene.mesh_mut(), v3(3., 0., 0.), v3(3.5, 1., 2.5)); // the post
    vp.scene.version += 1;
    vp.camera.set_view("front");
    vp.camera.perspective = false;
    vp.camera.fit_to(v3(-1., 0., -0.5), v3(4.5, 1., 3.), 1.3);
    vp.scene.clear_selection();
    for f in cube {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    w.activate_tool("scale");
    let vp = &mut w.viewport;
    let g = {
        let t = vp.tool_mut::<ScaleTool>().unwrap();
        grip(t, (0.5, 0.5, 1.0)) // top face grip
    };
    vp.with_tool(|t, vp| {
        let t = t.as_any_mut().downcast_mut::<ScaleTool>().unwrap();
        t.grab(vp, g, (0., 0.));
    });

    let hover = |vp: &mut Viewport, p: DVec3| -> Option<SnapResult> {
        let ScreenPt { x, y } = vp.world_to_pixel(p).unwrap();
        vp.mouse_move(&MouseEvent::at(x, y));
        vp.inference.last_snap.clone()
    };
    let s = hover(vp, v3(3.0, 0.0, 2.5)).expect("a snap"); // the post's corner
    assert_eq!(s.kind, SnapKind::Endpoint);
    let fz = vp.tool::<ScaleTool>().unwrap().factors[2];
    assert!((fz - 2.5).abs() < 1e-5, "{fz}");
    let own = hover(vp, v3(1.0, 0.0, fz));
    assert!(
        !own.as_ref()
            .is_some_and(|o| o.kind == SnapKind::Endpoint && (o.point.x - 1.0).abs() < 1e-6),
        "{own:?}"
    );
    vp.with_tool(|t, vp| t.on_cancel(vp));
}
