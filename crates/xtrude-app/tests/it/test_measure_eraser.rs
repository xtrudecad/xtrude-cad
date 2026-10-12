// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Guide entities and commands, Eraser strokes (the .igz round trip lives in
//! crates/xtrude-io).

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::eraser::EraserTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::commands::{AddGuideCommand, DeleteGuidesCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::guide::Guide;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::textlabel::TextLabel;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

// ---- Guide entity ------------------------------------------------------------

#[test]
fn test_guide_line_segment_spans_both_ways() {
    let g = Guide::new(v(1., 2.), Some(v3(1., 0., 0.)), None);
    let (a, b) = g.segment();
    assert!(a.x < 1.0 && 1.0 < b.x && (a.y - 2.).abs() < 1e-9 && (b.y - 2.).abs() < 1e-9);
    assert!(g.is_line());
    // A snap edge: a and b differ.
    assert!((g.b() - g.a()).length() > 1.0);
}

#[test]
fn test_guide_point() {
    let g = Guide::new(v(3., 4.), None, None);
    assert!(!g.is_line());
    let (a, b) = g.segment();
    assert!((a - b).length() < 1e-9);
}

#[test]
fn test_guide_commands_undo() {
    let mut vp = stub();
    let g = Guide::new(v(0., 0.), Some(v3(0., 1., 0.)), None);
    vp.execute(&mut AddGuideCommand::new(g.clone()));
    assert_eq!(vp.scene.doc.guides, vec![g.clone()]);
    assert!(vp.undo());
    assert!(vp.scene.doc.guides.is_empty());
    assert!(vp.redo());
    vp.execute(&mut DeleteGuidesCommand { ids: vec![g.id] });
    assert!(vp.scene.doc.guides.is_empty());
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.guides, vec![g]);
}

#[test]
fn test_clear_resets_guides() {
    let mut vp = stub();
    vp.scene
        .doc
        .guides
        .push(Guide::new(v(0., 0.), Some(v3(1., 0., 0.)), None));
    vp.scene.clear();
    assert!(vp.scene.doc.guides.is_empty());
}

// ---- Eraser stroke (headless: mark + release) --------------------------------

#[test]
fn test_eraser_release_erases_marked_edges_one_undo() {
    let mut vp = stub();
    let sq = [v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&vp.scene, &segs, true, vec![], None);
    vp.execute_boxed(cmd);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.marked = vp.scene.mesh().edge_ids()[..2]
        .iter()
        .map(|&e| EntityRef::Edge(MeshId::LOOSE, e))
        .collect(); // swept over two edges
    tool.on_release(&mut vp);
    assert_eq!(vp.scene.mesh().edges.len(), 2); // two gone in one step
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().edges.len(), 4); // single undo restores
}

#[test]
fn test_eraser_release_erases_guides() {
    let mut vp = stub();
    let g = Guide::new(v(0., 0.), Some(v3(1., 0., 0.)), None);
    let gid = g.id;
    vp.scene.doc.guides.push(g);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.marked = vec![EntityRef::Guide(gid)];
    tool.on_release(&mut vp);
    assert!(vp.scene.doc.guides.is_empty());
}

// ---- the eraser takes groups and components whole

fn grouped_square(vp: &mut Viewport) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("box"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    gid
}

/// Nothing loose under the cursor, a group there instead.
fn group_picking(vp: &mut Viewport, g: GroupId) {
    FakeProbe::of(vp).pick_edge(|_, _| None);
    FakeProbe::of(vp).pick_group(move |_, _| Some(g));
}

fn has_group(vp: &Viewport, g: GroupId) -> bool {
    vp.scene.doc.groups.iter().any(|x| x.id == g)
}

#[test]
fn test_the_eraser_marks_the_group_under_the_cursor_and_erases_it_whole() {
    let mut vp = stub();
    let g = grouped_square(&mut vp);
    group_picking(&mut vp, g);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.mark(&mut vp, 10.0, 10.0);
    assert_eq!(tool.marked, vec![EntityRef::Group(g)]);
    tool.on_release(&mut vp);
    assert!(!has_group(&vp, g));
    assert!(vp.undo());
    assert!(has_group(&vp, g));
}

#[test]
fn test_shift_stroke_hides_the_group_instead() {
    let mut vp = stub();
    let g = grouped_square(&mut vp);
    group_picking(&mut vp, g);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.hide = true;
    tool.mark(&mut vp, 10.0, 10.0);
    assert_eq!(tool.marked, vec![EntityRef::Group(g)]);
    tool.on_release(&mut vp);
    assert!(has_group(&vp, g) && vp.scene.doc.find_group(g).unwrap().hidden);
    assert!(vp.undo());
    assert!(!vp.scene.doc.find_group(g).unwrap().hidden);
}

// ---- the eraser erases texts

fn label_over_group(vp: &mut Viewport) -> (GroupId, TextLabel) {
    let g = grouped_square(vp);
    group_picking(vp, g);
    let lab = TextLabel::new(v(1., 1.), v3(1., 1., 1.), "Rib");
    vp.scene.doc.text_labels.push(lab.clone());
    let id = lab.id;
    FakeProbe::of(vp).pick_text_label(move |_, _, _| Some(id));
    (g, lab)
}

#[test]
fn test_the_eraser_erases_a_leader_text_before_what_is_behind_it() {
    let mut vp = stub();
    let (g, lab) = label_over_group(&mut vp);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.mark(&mut vp, 10.0, 10.0);
    assert_eq!(tool.marked, vec![EntityRef::TextLabel(lab.id)]); // the glyphs outrank the group
    tool.on_release(&mut vp);
    assert!(vp.scene.doc.text_labels.is_empty() && has_group(&vp, g));
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.text_labels, vec![lab]);
}

#[test]
fn test_a_hide_stroke_leaves_texts_alone() {
    let mut vp = stub();
    let (_g, lab) = label_over_group(&mut vp);
    let mut tool = EraserTool::new();
    tool.stroke = true;
    tool.hide = true;
    tool.mark(&mut vp, 10.0, 10.0);
    assert!(!tool.marked.contains(&EntityRef::TextLabel(lab.id)));
}
