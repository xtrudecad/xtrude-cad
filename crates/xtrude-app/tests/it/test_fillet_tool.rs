// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Click an edge, size the radius with the cursor or the value box, click to
//! round it — one undo step.

use crate::common;

use common::{edge_at, set_hover_edge, sized_viewport, v3, MetreDocument};
use glam::DVec3;
use xtrude_app::tools::fillet::{fillet_radius, set_fillet_radius, FilletTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::orient::is_closed;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

/// A 2 × 1 × 1 box with outward-wound faces.
fn box_scene(vp: &mut Viewport) {
    let (l, w, h) = (2.0, 1.0, 1.0);
    let loops = [
        [v3(0., 0., 0.), v3(0., w, 0.), v3(l, w, 0.), v3(l, 0., 0.)],
        [v3(0., 0., h), v3(l, 0., h), v3(l, w, h), v3(0., w, h)],
        [v3(0., 0., 0.), v3(l, 0., 0.), v3(l, 0., h), v3(0., 0., h)],
        [v3(l, 0., 0.), v3(l, w, 0.), v3(l, w, h), v3(l, 0., h)],
        [v3(l, w, 0.), v3(0., w, 0.), v3(0., w, h), v3(l, w, h)],
        [v3(0., w, 0.), v3(0., 0., 0.), v3(0., 0., h), v3(0., w, h)],
    ];
    for lp in loops {
        vp.scene.mesh_mut().add_face(&lp, &[]);
    }
    vp.scene.version += 1;
}

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

fn tool(vp: &Viewport) -> &FilletTool {
    vp.tool::<FilletTool>().expect("the fillet tool")
}

#[test]
fn test_click_edge_size_with_the_cursor_click_to_round() {
    let _m = MetreDocument::new();
    let mut vp = sized_viewport(1000.0, 700.0);
    box_scene(&mut vp);
    set_fillet_radius(0.1);
    vp.set_active_tool(Some(Box::new(FilletTool::new())));
    let e = edge_at(&vp, v3(0., 0., 1.), v3(2., 0., 1.));
    set_hover_edge(&mut vp, e);
    vp.with_tool(|t, vp| t.on_hover(vp, &ctx(v3(1., 0., 1.))));
    assert_eq!(tool(&vp).hover_edge, Some(e));
    assert_eq!(tool(&vp).preview_faces().len(), 8);
    vp.with_tool(|t, vp| t.on_click(vp, &ctx(v3(1., 0., 1.))));
    assert!(tool(&vp).sizing && tool(&vp).edges == vec![e]);
    vp.with_tool(|t, vp| t.on_hover(vp, &ctx(v3(1.0, 0.25, 1.0))));
    assert!((fillet_radius() - 0.25).abs() < 1e-9);
    let label = vp.with_tool(|t, vp| t.value_label(vp)).flatten().unwrap().0;
    assert!(label.starts_with("R 0.250"), "{label}");
    let depth = vp.history.undo_len();
    vp.with_tool(|t, vp| t.on_click(vp, &ctx(v3(1.0, 0.25, 1.0))));
    assert_eq!(vp.history.undo_len(), depth + 1);
    assert!(vp.scene.mesh().faces.len() == 14 && is_closed(vp.scene.mesh()));
    assert!(!tool(&vp).sizing && tool(&vp).edges.is_empty());
    vp.undo();
    assert!(vp.scene.mesh().faces.len() == 6 && vp.scene.mesh().edges.len() == 12);
    vp.redo();
    assert_eq!(vp.scene.mesh().faces.len(), 14);
}

#[test]
fn test_a_typed_radius_rounds_the_selected_edges() {
    let mut vp = sized_viewport(1000.0, 700.0);
    box_scene(&mut vp);
    let rim = [
        edge_at(&vp, v3(0., 0., 1.), v3(2., 0., 1.)),
        edge_at(&vp, v3(2., 0., 1.), v3(2., 1., 1.)),
        edge_at(&vp, v3(2., 1., 1.), v3(0., 1., 1.)),
        edge_at(&vp, v3(0., 1., 1.), v3(0., 0., 1.)),
    ];
    vp.scene.select(
        rim.iter().map(|&e| EntityRef::Edge(MeshId::LOOSE, e)),
        SelectMode::Replace,
    );
    vp.set_active_tool(Some(Box::new(FilletTool::new())));
    let got: std::collections::BTreeSet<_> = tool(&vp).edges.iter().copied().collect();
    assert_eq!(got, rim.iter().copied().collect());
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Segments(6))),
        Some(true)
    );
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(0.15))),
        Some(true)
    );
    assert!(vp.scene.mesh().faces.len() == 6 + 4 * 6 && is_closed(vp.scene.mesh()));
    assert!(vp.scene.selection.is_empty());
    assert!((fillet_radius() - 0.15).abs() < 1e-9);
}

#[test]
fn test_a_refused_rounding_only_speaks() {
    let mut vp = sized_viewport(1000.0, 700.0);
    box_scene(&mut vp);
    vp.set_active_tool(Some(Box::new(FilletTool::new())));
    let e = edge_at(&vp, v3(0., 0., 1.), v3(2., 0., 1.));
    set_hover_edge(&mut vp, e);
    vp.with_tool(|t, vp| t.on_hover(vp, &ctx(v3(1., 0., 1.))));
    vp.with_tool(|t, vp| t.on_click(vp, &ctx(v3(1., 0., 1.))));
    vp.take_events();
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(-1.0))),
        Some(false)
    );
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(5.0))),
        Some(true)
    );
    assert_eq!(vp.scene.mesh().faces.len(), 6);
    let said: Vec<String> = vp
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect();
    assert!(
        said.iter()
            .any(|s| s.contains("too large") || s.contains("collapse")),
        "{said:?}"
    );
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert!(tool(&vp).edges.is_empty());
}
