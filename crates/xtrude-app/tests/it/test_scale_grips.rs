// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Scale tool's grip box — 26 grips on a 3D selection, 8 on a flat one;
//! anchors; typed factors, per-axis factors, absolute sizes and mirrors.

use crate::common;

use std::collections::HashMap;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::scale::{Grip, ScaleTool};
use xtrude_app::tools::{Tool, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::mt19937::Mt19937;
use xtrude_model::commands::{ScaleFactor, ScaleVerticesCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

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
    vp.scene
        .selection
        .insert(EntityRef::Face(MeshId::LOOSE, f1));
    vp.scene
        .selection
        .insert(EntityRef::Face(MeshId::LOOSE, f2));
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    (t, vp)
}

fn grip(t: &ScaleTool, p: (f64, f64, f64)) -> Grip {
    t.grips.iter().find(|g| g.params == p).cloned().unwrap()
}

fn max_x(vp: &Viewport) -> f64 {
    vp.scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MIN, f64::max)
}

fn min_x(vp: &Viewport) -> f64 {
    vp.scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MAX, f64::min)
}

#[test]
fn test_a_3d_selection_gets_the_classic_26_grips() {
    let (t, _) = tool_with_box(4., 3., 2.);
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for g in &t.grips {
        *kinds.entry(g.kind(3)).or_default() += 1;
    }
    assert_eq!(t.grips.len(), 26);
    assert_eq!(
        kinds,
        [("corner", 8), ("edge", 12), ("face", 6)]
            .into_iter()
            .collect()
    );
}

#[test]
fn test_a_flat_selection_gets_the_2d_box_of_8_grips() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 3., 0.),
            v3(0., 3., 0.),
        ],
        &[],
    );
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    assert_eq!(t.grips.len(), 8);
    let kinds: Vec<&str> = t.grips.iter().map(|g| g.kind(2)).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "corner").count(), 4);
    assert_eq!(kinds.iter().filter(|k| **k == "face").count(), 4);
}

#[test]
fn test_a_single_edge_gets_two_end_grips() {
    let mut vp = stub();
    let e = vp
        .scene
        .mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(5., 0., 0.))
        .unwrap();
    vp.scene.selection.insert(EntityRef::Edge(MeshId::LOOSE, e));
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    assert_eq!(t.grips.len(), 2);
    assert!(t.grips.iter().all(|g| g.mask == vec![0]));
}

#[test]
fn test_face_grips_scale_one_axis_edge_grips_two_corner_grips_all() {
    let (t, _) = tool_with_box(4., 3., 2.);
    for g in &t.grips {
        let halves = [g.params.0, g.params.1, g.params.2]
            .iter()
            .filter(|x| **x == 0.5)
            .count();
        assert_eq!(g.mask.len(), 3 - halves);
    }
}

#[test]
fn test_the_anchor_is_the_opposite_side() {
    let (t, _) = tool_with_box(4., 3., 2.);
    assert_eq!(t.anchor_for(&grip(&t, (1., 1., 1.))), v3(0., 0., 0.));
    assert_eq!(t.anchor_for(&grip(&t, (1., 0.5, 0.5))), v3(0., 1.5, 1.0));
}

#[test]
fn test_about_center_moveses_the_anchor_to_the_middle() {
    let (mut t, _) = tool_with_box(4., 3., 2.);
    t.about_center = true;
    assert_eq!(t.anchor_for(&grip(&t, (1., 1., 1.))), v3(2.0, 1.5, 1.0));
}

fn armed() -> (ScaleTool, Viewport) {
    let (mut t, vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (1.0, 0.5, 0.5));
    t.anchor = Some(t.anchor_for(&g));
    t.grip = Some(g);
    (t, vp)
}

#[test]
fn test_a_plain_number_is_a_factor_on_the_grips_axes() {
    let (t, _) = armed();
    assert_eq!(t.typed_factors_now(&[2.0], false), Some([2.0, 1.0, 1.0]));
}

#[test]
fn test_a_number_with_units_is_the_new_absolute_size() {
    let (t, _) = armed();
    assert_eq!(t.typed_factors_now(&[2.0], true), Some([0.5, 1.0, 1.0]));
}

#[test]
fn test_a_negative_factor_mirrors() {
    let (t, _) = armed();
    assert_eq!(t.typed_factors_now(&[-1.0], false), Some([-1.0, 1.0, 1.0]));
}

#[test]
fn test_zero_is_rejected() {
    let (t, _) = armed();
    assert_eq!(t.typed_factors_now(&[0.0], false), None);
}

#[test]
fn test_per_axis_factors_on_an_edge_grip() {
    let (mut t, _) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (1.0, 1.0, 0.5));
    t.anchor = Some(t.anchor_for(&g));
    t.grip = Some(g);
    assert_eq!(
        t.typed_factors_now(&[2.0, 3.0], false),
        Some([2.0, 3.0, 1.0])
    );
    assert_eq!(
        t.typed_factors_now(&[8.0, 6.0], true),
        Some([2.0, 2.0, 1.0])
    );
    assert_eq!(t.typed_factors_now(&[2.0, 3.0, 4.0], false), None);
}

#[test]
fn test_uniform_toggle_makes_any_grip_proportional() {
    let (mut t, _) = armed();
    t.uniform = true;
    assert_eq!(t.typed_factors_now(&[2.0], false), Some([2.0, 2.0, 2.0]));
}

#[test]
fn test_committed_scale_is_per_axis_and_undoable() {
    let mut vp = stub();
    let pts = vec![
        v3(0., 0., 0.),
        v3(4., 0., 0.),
        v3(4., 3., 0.),
        v3(0., 3., 0.),
    ];
    let f = vp.scene.mesh_mut().add_face(&pts, &[]);
    vp.run(ScaleVerticesCommand {
        src: pts,
        center: DVec3::ZERO,
        factor: ScaleFactor::PerAxis([2.0, 0.5, 1.0]),
        axes: None,
    });
    let sorted = |vp: &Viewport, i: usize| {
        let mut v: Vec<i64> = vp
            .scene
            .mesh()
            .face_positions(f)
            .iter()
            .map(|p| (p[i] * 1e6).round() as i64)
            .collect();
        v.sort();
        v
    };
    assert_eq!(sorted(&vp, 0), vec![0, 0, 8_000_000, 8_000_000]);
    assert_eq!(sorted(&vp, 1), vec![0, 0, 1_500_000, 1_500_000]);
    vp.undo();
    assert_eq!(sorted(&vp, 0), vec![0, 0, 4_000_000, 4_000_000]);
}

#[test]
fn test_dragging_a_face_grip_live_then_cancel_restores_geometry() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (1.0, 0.5, 0.5));
    t.grab(&mut vp, g, (0.0, 0.0));
    t.apply_preview(&mut vp, [2.0, 1.0, 1.0]);
    assert_eq!(max_x(&vp), 8.0);
    t.on_cancel(&mut vp);
    assert_eq!(max_x(&vp), 4.0);
    assert_eq!(vp.history.undo_len(), 0);
}

#[test]
fn test_mirror_by_preview_through_the_anchor() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (1.0, 0.5, 0.5));
    t.grab(&mut vp, g, (0.0, 0.0));
    t.apply_preview(&mut vp, [-1.0, 1.0, 1.0]);
    assert_eq!(min_x(&vp), -4.0);
    t.on_cancel(&mut vp);
    assert_eq!(min_x(&vp), 0.0);
}

#[test]
fn test_hot_retype_redoes_the_last_scale_at_the_new_factor() {
    let (mut t, mut vp) = tool_with_box(4., 3., 2.);
    let g = grip(&t, (1.0, 0.5, 0.5));
    t.grab(&mut vp, g, (0.0, 0.0));
    t.commit(&mut vp, [2.0, 1.0, 1.0]);
    assert_eq!(max_x(&vp), 8.0);
    assert!(t.on_value(&mut vp, &ToolValue::Num(3.0)));
    assert_eq!(max_x(&vp), 12.0);
    assert_eq!(vp.history.undo_len(), 1);
}

#[test]
fn test_value_box_captions_follow_the_axes() {
    let (mut t, _) = tool_with_box(4., 3., 2.);
    assert_eq!(t.value_box_caption().as_deref(), Some("Scale"));
    t.grip = Some(grip(&t, (1.0, 0.5, 0.5)));
    assert_eq!(t.value_box_caption().as_deref(), Some("Red Scale"));
    t.grip = Some(grip(&t, (1.0, 1.0, 0.5)));
    assert_eq!(t.value_box_caption().as_deref(), Some("Red, Green Scale"));
    t.grip = Some(grip(&t, (1.0, 1.0, 1.0)));
    assert_eq!(t.value_box_caption().as_deref(), Some("Scale"));
}

#[test]
fn test_a_long_drag_down_and_back_scales_every_vertex() {
    let mut vp = stub();
    for i in 0..30 {
        let x0 = 3.1234 + i as f64 * 0.0137;
        let f = vp.scene.mesh_mut().add_face(
            &[
                v3(x0, 1.777, 0.7),
                v3(x0 + 0.0137, 1.777, 0.7),
                v3(x0 + 0.0137, 1.9, 0.9),
                v3(x0, 1.9, 0.9),
            ],
            &[],
        );
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    let mut t = ScaleTool::new();
    t.on_activate(&mut vp);
    let g = t
        .grips
        .iter()
        .max_by(|a, b| {
            let ka = (a.params.0, a.params.1, a.params.2);
            let kb = (b.params.0, b.params.1, b.params.2);
            ka.partial_cmp(&kb).unwrap()
        })
        .cloned()
        .unwrap();
    let before: Vec<(xtrude_core::VertexId, DVec3)> = vp
        .scene
        .mesh()
        .verts
        .iter()
        .map(|(id, v)| (id, v.position))
        .collect();
    t.grab(&mut vp, g, (0.0, 0.0));
    let anchor = t.anchor.unwrap();
    let mut rnd = Mt19937::new(1);
    let choices = [0.04, 0.09, 0.2, 0.56, 0.93, 1.11, 1.2, 0.15];
    for _ in 0..300 {
        let f = choices[rnd.choice_index(choices.len())];
        t.apply_preview(&mut vp, [f, f, f]);
    }
    t.apply_preview(&mut vp, [0.5, 0.5, 0.5]);
    t.commit(&mut vp, [0.5, 0.5, 0.5]);
    let m = vp.scene.mesh();
    for (id, p0) in &before {
        let want = anchor + (*p0 - anchor) * 0.5;
        assert!((m.pos(*id) - want).length() < 1e-5);
    }
    assert_eq!(m.verts.len(), before.len());
}
