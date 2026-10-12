// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotate lands exactly where an inference held the cursor (not on the label's
//! 0.1°), and the live preview leaves no drift behind.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;

/// What the guide line's end really is.
const TARGET_DEG: f64 = -70.2789;
const R: f64 = 2.0;

fn ctx(p: DVec3, kind: Option<SnapKind>) -> ToolContext {
    let mut c = ToolContext::at(p);
    c.snap = kind.map(|k| SnapResult::new(p, k, [1., 1., 1.]));
    c
}

fn target() -> DVec3 {
    let t = TARGET_DEG.to_radians();
    v3(R * t.cos(), R * t.sin(), 0.)
}

/// A thin panel whose far corner sits at (R, 0).
fn panel(mesh: &mut Mesh) -> xtrude_core::FaceId {
    mesh.add_face(
        &[
            v3(0., 0., 0.),
            v3(R, 0., 0.),
            v3(R, 0.1, 0.),
            v3(0., 0.1, 0.),
        ],
        &[],
    )
}

/// World position of the vertex that started at (R, 0).
fn far_corner(mesh: &Mesh, xform: Option<glam::DMat4>) -> DVec3 {
    let mut best: Option<DVec3> = None;
    for (_, vx) in mesh.verts.iter() {
        let p = xform.map_or(vx.position, |x| x.transform_point3(vx.position));
        if (p.length() - R).abs() < 1e-3 && best.is_none_or(|b| p.z.abs() <= b.z.abs()) {
            best = Some(p);
        }
    }
    best.expect("the far corner")
}

fn swing(vp: &mut Viewport, tool: &mut RotateTool, final_kind: Option<SnapKind>) {
    tool.on_click(vp, &ctx(DVec3::ZERO, None)); // pivot
    tool.on_click(vp, &ctx(v3(R, 0., 0.), None)); // reference arm
    for k in 1..60 {
        let t = (TARGET_DEG * k as f64 / 60.0 + 3.0 * (k as f64).sin()).to_radians();
        tool.on_hover(vp, &ctx(v3(R * t.cos(), R * t.sin(), 0.), None));
    }
    tool.on_hover(vp, &ctx(target(), final_kind.clone()));
    tool.on_click(vp, &ctx(target(), final_kind));
}

fn loose_panel() -> Viewport {
    let mut vp = stub();
    let f = panel(vp.scene.mesh_mut());
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    vp
}

#[test]
fn test_loose_geometry_lands_exactly_on_the_snapped_endpoint() {
    let mut vp = loose_panel();
    swing(&mut vp, &mut RotateTool::new(), Some(SnapKind::Endpoint));
    let corner = far_corner(vp.scene.mesh(), None);
    assert!((corner - target()).length() < 2e-6, "{corner} {}", target());
}

#[test]
fn test_a_group_lands_exactly_on_the_snapped_endpoint() {
    let mut vp = stub();
    let mut m = Mesh::new();
    panel(&mut m);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("Panel"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.scene.selection.insert(EntityRef::Group(gid));
    swing(&mut vp, &mut RotateTool::new(), Some(SnapKind::Endpoint));
    let g = vp.scene.doc.groups.iter().find(|g| g.id == gid).unwrap();
    let corner = far_corner(vp.scene.doc.mesh(g.mesh), g.xform);
    assert!((corner - target()).length() < 2e-6, "{corner} {}", target());
}

#[test]
fn test_a_free_cursor_still_rotates_by_the_rounded_angle() {
    let mut vp = loose_panel();
    swing(&mut vp, &mut RotateTool::new(), None);
    let corner = far_corner(vp.scene.mesh(), None);
    let applied = corner.y.atan2(corner.x).to_degrees();
    let want = (TARGET_DEG * 10.0).round() / 10.0;
    assert!((applied - want).abs() < 1e-4, "{applied}");
}

#[test]
fn test_cancelling_after_a_long_drag_puts_everything_back_exactly() {
    let mut vp = loose_panel();
    let positions = |vp: &Viewport| -> Vec<DVec3> {
        vp.scene
            .mesh()
            .verts
            .iter()
            .map(|(_, v)| v.position)
            .collect()
    };
    let before = positions(&vp);
    let mut tool = RotateTool::new();
    tool.on_click(&mut vp, &ctx(DVec3::ZERO, None));
    tool.on_click(&mut vp, &ctx(v3(R, 0., 0.), None));
    for k in 0..400 {
        let k = k as f64;
        let t = (37.0 * (k * 0.37).sin() + 0.013 * k).to_radians();
        tool.on_hover(&mut vp, &ctx(v3(R * t.cos(), R * t.sin(), 0.), None));
    }
    tool.on_cancel(&mut vp);
    assert_eq!(positions(&vp), before, "the drag left drift");
}

#[test]
fn test_a_typed_angle_is_applied_exactly() {
    let mut vp = loose_panel();
    let mut tool = RotateTool::new();
    tool.on_click(&mut vp, &ctx(DVec3::ZERO, None));
    tool.on_click(&mut vp, &ctx(v3(R, 0., 0.), None));
    for k in 0..30 {
        let t = (k as f64 * 1.7).to_radians();
        tool.on_hover(&mut vp, &ctx(v3(R * t.cos(), R * t.sin(), 0.), None));
    }
    tool.on_value(&mut vp, &ToolValue::Num(30.0));
    let corner = far_corner(vp.scene.mesh(), None);
    assert!((corner.y.atan2(corner.x).to_degrees() - 30.0).abs() < 1e-5);
}

#[test]
fn test_the_protractor_guide_passes_through_the_snapped_point() {
    let mut tool = ProtractorTool::new();
    tool.p.start_point = Some(DVec3::ZERO);
    tool.p.ref_point = Some(v3(R, 0., 0.));
    tool.p.exact_snap = true;
    tool.p.snap_ticks = false;
    assert!((tool.p.commit_deg(target()).unwrap() - TARGET_DEG).abs() < 1e-5);
    tool.p.exact_snap = false;
    assert_eq!(
        tool.p.commit_deg(target()),
        Some((TARGET_DEG * 10.0).round() / 10.0)
    );
}
