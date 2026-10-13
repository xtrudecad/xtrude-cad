// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Click an edge, size the distance with the cursor or the value box,
//! click to chamfer it — one undo step.

use crate::common;

use common::{add_box, edge_at, set_hover_edge, sized_viewport, v3};
use glam::DVec3;
use xtrude_app::tools::chamfer::{chamfer_distance, set_chamfer_distance, ChamferTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_app::window::hints::hint_for;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

const MM: f64 = 0.001;

/// A 10 mm cube.
fn cube(vp: &mut Viewport) {
    add_box(vp, DVec3::ZERO, v3(10., 10., 10.) * MM);
    vp.scene.version += 1;
}

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

fn tool(vp: &Viewport) -> &ChamferTool {
    vp.tool::<ChamferTool>().expect("the chamfer tool")
}

fn flashes(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn test_click_an_edge_size_with_the_cursor_click_to_chamfer() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    set_chamfer_distance(1. * MM);
    vp.set_active_tool(Some(Box::new(ChamferTool::new())));
    let e = edge_at(&vp, v3(0., 0., 10.) * MM, v3(10., 0., 10.) * MM);
    set_hover_edge(&mut vp, e);
    let on_edge = v3(5., 0., 10.) * MM;
    vp.with_tool(|t, vp| t.on_hover(vp, &ctx(on_edge)));
    assert_eq!(tool(&vp).hover_edge, Some(e));
    // the preview: one flat bevel
    assert_eq!(tool(&vp).preview_faces().len(), 1);
    vp.with_tool(|t, vp| t.on_click(vp, &ctx(on_edge)));
    assert!(tool(&vp).sizing && tool(&vp).edges == vec![e]);
    assert!(hint_for(Some("chamfer"), vp.active_tool(), None, None).contains("distance"));
    // 2 mm from the edge
    vp.with_tool(|t, vp| t.on_hover(vp, &ctx(v3(5., 2., 10.) * MM)));
    assert!((chamfer_distance() - 2. * MM).abs() < 1e-12);
    let depth = vp.history.undo_len();
    vp.with_tool(|t, vp| t.on_click(vp, &ctx(v3(5., 2., 10.) * MM)));
    assert_eq!(vp.history.undo_len(), depth + 1);
    let m = vp.scene.mesh();
    assert!(m.faces.len() == 7 && is_closed(m));
    let v = signed_volume(m) / MM.powi(3);
    assert!((v - 980.).abs() < 1e-6, "{v}");
    assert!(!tool(&vp).sizing && tool(&vp).edges.is_empty());
    vp.undo();
    assert!(vp.scene.mesh().faces.len() == 6 && vp.scene.mesh().edges.len() == 12);
    vp.redo();
    assert_eq!(vp.scene.mesh().faces.len(), 7);
}

#[test]
fn test_a_typed_distance_chamfers_the_selected_edges() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    let all = vp.scene.mesh().edge_ids();
    vp.scene.select(
        all.iter().map(|&e| EntityRef::Edge(MeshId::LOOSE, e)),
        SelectMode::Replace,
    );
    vp.set_active_tool(Some(Box::new(ChamferTool::new())));
    assert_eq!(tool(&vp).edges.len(), 12);
    let depth = vp.history.undo_len();
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(2. * MM))),
        Some(true)
    );
    assert_eq!(vp.history.undo_len(), depth + 1);
    let m = vp.scene.mesh();
    assert!(m.faces.len() == 6 + 12 + 8 && is_closed(m));
    assert!(vp.scene.selection.is_empty());
}

#[test]
fn test_a_refused_chamfer_says_why() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    let all = vp.scene.mesh().edge_ids();
    vp.scene.select(
        all.iter().map(|&e| EntityRef::Edge(MeshId::LOOSE, e)),
        SelectMode::Replace,
    );
    vp.set_active_tool(Some(Box::new(ChamferTool::new())));
    vp.take_events();
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(6. * MM))),
        Some(true)
    );
    assert_eq!(vp.scene.mesh().faces.len(), 6);
    let said = flashes(&mut vp);
    assert!(
        said.iter().any(|s| s.contains("distance is too large")),
        "{said:?}"
    );
    let label = vp.with_tool(|t, vp| t.value_label(vp)).flatten().unwrap().0;
    assert!(label.contains("distance"), "{label}");
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert!(tool(&vp).edges.is_empty());
}
