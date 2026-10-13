// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The reversible hide of edges and the Shift+eraser stroke, and the drawn
//! edges following a hide. The document test lives in crates/xtrude-io.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::eraser::EraserTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_core::EdgeId;
use xtrude_model::commands::HideCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_render::frame::{Source, Stipple};

fn square() -> Viewport {
    let mut vp = stub();
    let sq = [
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 2., 0.),
        v3(0., 2., 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    vp
}

fn hide(vp: &mut Viewport, edges: &[EdgeId], hidden: bool) {
    vp.execute(&mut HideCommand {
        entities: edges
            .iter()
            .map(|&e| EntityRef::Edge(MeshId::LOOSE, e))
            .collect(),
        hidden,
    });
}

fn hidden(vp: &Viewport, e: EdgeId) -> bool {
    vp.scene.mesh().edge(e).hidden
}

#[test]
fn test_hide_and_undo_restores_mixed_states() {
    let mut vp = square();
    let ids = vp.scene.mesh().edge_ids();
    let (e0, e1) = (ids[0], ids[1]);
    vp.scene.mesh_mut().edge_mut(e1).hidden = true; // already hidden (mixed)
    hide(&mut vp, &[e0, e1], true);
    assert!(hidden(&vp, e0) && hidden(&vp, e1));
    assert!(vp.undo());
    assert!(!hidden(&vp, e0)); // back to visible
    assert!(hidden(&vp, e1)); // was hidden before: stays hidden
}

#[test]
fn test_unhide_and_redo() {
    let mut vp = square();
    let edges: Vec<EdgeId> = vp.scene.mesh().edge_ids()[..2].to_vec();
    hide(&mut vp, &edges, true);
    hide(&mut vp, &edges, false);
    assert!(!edges.iter().any(|&e| hidden(&vp, e)));
    assert!(vp.undo());
    assert!(edges.iter().all(|&e| hidden(&vp, e)));
    assert!(vp.redo());
    assert!(!edges.iter().any(|&e| hidden(&vp, e)));
}

#[test]
fn test_hiding_drops_edges_from_selection() {
    let mut vp = square();
    let e0 = vp.scene.mesh().edge_ids()[0];
    vp.scene
        .selection
        .insert(EntityRef::Edge(MeshId::LOOSE, e0));
    hide(&mut vp, &[e0], true);
    assert!(!vp
        .scene
        .selection
        .contains(&EntityRef::Edge(MeshId::LOOSE, e0)));
    // The face survives untouched: hiding is a render flag, not topology.
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert_eq!(vp.scene.mesh().edges.len(), 4);
}

#[test]
fn test_eraser_shift_stroke_hides_instead_of_erasing() {
    let mut vp = square();
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.hide = true; // Shift was down at press
    tool.marked = vp.scene.mesh().edge_ids()[..2]
        .iter()
        .map(|&e| EntityRef::Edge(MeshId::LOOSE, e))
        .collect();
    tool.on_release(&mut vp);
    let m = vp.scene.mesh();
    assert_eq!(m.edges.len(), 4); // nothing erased
    assert_eq!(m.faces.len(), 1); // the face is still there
    assert_eq!(
        m.edge_ids().iter().filter(|&&e| m.edge(e).hidden).count(),
        2
    );
    assert!(vp.undo()); // one undo step for the stroke
    let m = vp.scene.mesh();
    assert!(!m.edge_ids().iter().any(|&e| m.edge(e).hidden));
}

/// The edges the next frame draws as solid lines, rounded.
fn drawn_edges(vp: &mut Viewport) -> Vec<[i64; 4]> {
    let f = vp.frame_data([200, 200], false);
    let o = f.origin;
    let mut out = Vec::new();
    for d in f
        .draws
        .iter()
        .filter(|d| d.source == Source::SceneLines && d.stipple == Stipple::None)
    {
        let range = d.first as usize..(d.first + d.count) as usize;
        for seg in f.scene_lines.data[range].chunks_exact(2) {
            let p = |i: usize| {
                let q = seg[i].pos;
                [
                    ((o.x + q[0] as f64) * 1000.).round() as i64,
                    ((o.y + q[1] as f64) * 1000.).round() as i64,
                ]
            };
            let ([ax, ay], [bx, by]) = (p(0), p(1));
            out.push([ax, ay, bx, by]);
        }
    }
    out.sort();
    out
}

#[test]
fn test_fingerprint_sees_hidden_edges() {
    // A hide toggle must invalidate the cached drawing — and by WHICH edge
    // is hidden, not how many ("hide A, undo, hide B" is not unchanged).
    // The frame cache key carries the active mesh's mutation serial; the
    // drawn edges are compared.
    let mut vp = square();
    let edges = vp.scene.mesh().edge_ids();
    let fp0 = drawn_edges(&mut vp);
    assert_eq!(fp0.len(), 4);

    vp.scene.mesh_mut().edge_mut(edges[0]).hidden = true;
    let fp_a = drawn_edges(&mut vp);
    assert_ne!(fp_a, fp0);

    vp.scene.mesh_mut().edge_mut(edges[0]).hidden = false;
    vp.scene.mesh_mut().edge_mut(edges[1]).hidden = true; // same count, different edge
    let fp_b = drawn_edges(&mut vp);
    assert!(fp_b != fp_a && fp_b != fp0);

    vp.scene.mesh_mut().edge_mut(edges[1]).hidden = false;
    assert_eq!(drawn_edges(&mut vp), fp0);
}
