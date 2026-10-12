// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Leader text (3D text geometry lives in crates/xtrude-core, the .igz round
//! trip in crates/xtrude-io).

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::fontmetrics::FontMetrics;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_model::commands::{
    AddTextLabelCommand, DeleteTextLabelsCommand, EditTextLabelCommand, MoveTextLabelsCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::ObjId;
use xtrude_model::scene::SelectMode;
use xtrude_model::textlabel::TextLabel;

fn rib_label() -> TextLabel {
    TextLabel::new(v3(1., 2., 0.), v3(0.5, 0., 1.), "Rib A")
}

fn label(vp: &Viewport, id: ObjId) -> &TextLabel {
    vp.scene
        .doc
        .text_labels
        .iter()
        .find(|l| l.id == id)
        .unwrap()
}

#[test]
fn test_label_commands_undo_redo() {
    let mut vp = stub();
    let lab = rib_label();
    vp.execute(&mut AddTextLabelCommand {
        label: Some(lab.clone()),
    });
    assert_eq!(vp.scene.doc.text_labels, vec![lab.clone()]);
    assert!(vp.undo());
    assert!(vp.scene.doc.text_labels.is_empty());
    assert!(vp.redo());
    assert_eq!(vp.scene.doc.text_labels, vec![lab.clone()]);
    vp.execute(&mut DeleteTextLabelsCommand { ids: vec![lab.id] });
    assert!(vp.scene.doc.text_labels.is_empty());
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.text_labels, vec![lab]);
}

#[test]
fn test_label_edit_command_undo_redo() {
    let mut vp = stub();
    let lab = rib_label();
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    vp.execute(&mut EditTextLabelCommand {
        id,
        text: "Rib B".into(),
    });
    assert_eq!(label(&vp, id).text, "Rib B");
    assert!(vp.undo());
    assert_eq!(label(&vp, id).text, "Rib A");
    assert!(vp.redo());
    assert_eq!(label(&vp, id).text, "Rib B");
}

#[test]
fn test_select_double_click_edits_label() {
    let mut vp = stub();
    let lab = rib_label();
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    FakeProbe::of(&mut vp).pick_text_label(move |_, _, _| Some(id));
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    vp.prompt = Some(Box::new(|_, _, _, _| Some("Rib B".into())));
    SelectTool::new().on_double_click(&mut vp, &ToolContext::at(DVec3::ZERO));
    assert_eq!(label(&vp, id).text, "Rib B");
    assert!(vp.undo());
    assert_eq!(label(&vp, id).text, "Rib A");
    // cancelling the dialog leaves the label untouched (and no undo entry)
    let steps = vp.history.undo_len();
    vp.prompt = Some(Box::new(|_, _, _, _| None));
    SelectTool::new().on_double_click(&mut vp, &ToolContext::at(DVec3::ZERO));
    assert_eq!(label(&vp, id).text, "Rib A");
    assert_eq!(vp.history.undo_len(), steps);
}

#[test]
fn test_label_move_command_shifts_offset_only() {
    let mut vp = stub();
    let lab = rib_label();
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    vp.execute(&mut MoveTextLabelsCommand {
        ids: vec![id],
        delta: v3(1., 0., 2.),
    });
    assert!((label(&vp, id).anchor - v3(1., 2., 0.)).length() < 1e-9); // pinned
    assert!((label(&vp, id).offset - v3(1.5, 0., 3.)).length() < 1e-9);
    assert!(vp.undo());
    assert!((label(&vp, id).offset - v3(0.5, 0., 1.)).length() < 1e-9);
}

#[test]
fn test_move_tool_drags_selected_label() {
    let mut vp = stub();
    let lab = rib_label();
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    vp.scene
        .select([EntityRef::TextLabel(id)], SelectMode::Replace);
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    FakeProbe::of(&mut vp).pick_text_label(|_, _, _| None);
    let mut tool = MoveTool::new();
    tool.on_click(&mut vp, &ToolContext::at(v3(0., 0., 0.)));
    assert_eq!(tool.labels, vec![id]);
    tool.on_click(&mut vp, &ToolContext::at(v3(2., 0., 1.)));
    assert!((label(&vp, id).anchor - v3(1., 2., 0.)).length() < 1e-9);
    assert!((label(&vp, id).offset - v3(2.5, 0., 2.)).length() < 1e-9);
    assert!(vp.undo());
    assert!((label(&vp, id).offset - v3(0.5, 0., 1.)).length() < 1e-9);
}

#[test]
fn test_select_delete_key_removes_label() {
    let mut vp = stub();
    let lab = rib_label();
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab.clone());
    vp.scene
        .select([EntityRef::TextLabel(id)], SelectMode::Replace);
    assert!(SelectTool::new().on_key(&mut vp, Key::Delete, Modifiers::NONE));
    assert!(vp.scene.doc.text_labels.is_empty());
    assert!(vp.scene.selection.is_empty());
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.text_labels, vec![lab]);
}

#[test]
fn test_text_block_sits_away_from_the_anchor() {
    let x = Viewport::text_block_x;
    assert_eq!(x((100.0, 50.0), Some((20.0, 50.0)), 80.0), 106.0); // anchor left
    assert_eq!(x((100.0, 50.0), Some((300.0, 50.0)), 80.0), 14.0); // anchor right
    assert_eq!(x((100.0, 50.0), None, 80.0), 106.0); // no anchor
}

#[test]
fn test_pick_text_label_hits_the_drawn_block() {
    let mut vp = stub();
    let lab = TextLabel::new(v3(3., 0., 0.), v3(-2., 0., 0.), "Socket");
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    FakeProbe::of(&mut vp).world_to_pixel(|p| {
        if (p.x - 3.0).abs() < 1e-6 {
            Some((300.0, 100.0))
        } else if (p.x - 1.0).abs() < 1e-6 {
            Some((100.0, 100.0))
        } else {
            None
        }
    });
    let w = FontMetrics::new(9.0, true).horizontal_advance("Socket");
    assert_eq!(vp.pick_text_label(100. - 6. - w / 2., 96., true), Some(id)); // inside block
    assert_eq!(vp.pick_text_label(100. + 6. + w / 2., 96., true), None); // old side
}

#[test]
fn test_place_face_frame_maps_walls_and_floors() {
    use xtrude_app::tools::place_group::PlaceGroupTool;
    // wall facing -Y: the sign stands upright, reading along +X
    let (right, y_axis, up) = PlaceGroupTool::face_frame(v3(0., -1., 0.));
    assert!((right - v3(1., 0., 0.)).length() < 1e-6);
    assert!((up - v3(0., 0., 1.)).length() < 1e-6);
    assert!((y_axis - v3(0., 1., 0.)).length() < 1e-6); // front -> -Y = normal
                                                        // floor facing +Z: the text lies flat, its up pointing north
    let (right, y_axis, up) = PlaceGroupTool::face_frame(v3(0., 0., 1.));
    assert!((right - v3(1., 0., 0.)).length() < 1e-6);
    assert!((up - v3(0., 1., 0.)).length() < 1e-6);
    assert!((y_axis - v3(0., 0., -1.)).length() < 1e-6); // front faces up
}
