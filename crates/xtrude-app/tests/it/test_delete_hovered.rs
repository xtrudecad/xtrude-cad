// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Delete erases what is under the cursor with no click; a selection still
//! wins; Backspace never takes the hover; tools holding geometry wait.

use crate::common;

use common::{draw_rect, stub, v3};
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::offset::OffsetTool;
use xtrude_app::tools::select::{delete_selection_or_hover, SelectTool};
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn square() -> Viewport {
    let mut vp = stub();
    draw_rect(
        &mut vp,
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
    );
    vp
}

fn face(vp: &Viewport) -> EntityRef {
    EntityRef::Face(MeshId::LOOSE, vp.scene.mesh().face_ids()[0])
}

fn edge(vp: &Viewport) -> EntityRef {
    EntityRef::Edge(MeshId::LOOSE, vp.scene.mesh().edge_ids()[0])
}

fn exists(vp: &Viewport, e: EntityRef) -> bool {
    match e {
        EntityRef::Face(_, f) => vp.scene.mesh().has_face(f),
        EntityRef::Edge(_, x) => vp.scene.mesh().has_edge(x),
        _ => false,
    }
}

#[test]
fn test_delete_erases_the_hovered_face_without_a_click() {
    let mut vp = square();
    let f = face(&vp);
    let n_edges = vp.scene.mesh().edges.len();
    vp.set_hover(Some(f));
    assert!(SelectTool::new().on_key(&mut vp, Key::Delete, Modifiers::NONE));
    assert!(!exists(&vp, f));
    assert_eq!(vp.scene.mesh().edges.len(), n_edges); // the face alone goes
    assert!(vp.hover_entity().is_none()); // no dangling highlight
    assert!(vp.undo()); // one step
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_delete_erases_the_hovered_edge_and_its_face() {
    let mut vp = square();
    let e = edge(&vp);
    vp.set_hover(Some(e));
    assert!(delete_selection_or_hover(&mut vp));
    assert!(!exists(&vp, e));
    assert!(vp.scene.mesh().faces.is_empty()); // the face follows
}

#[test]
fn test_a_selection_wins_over_the_hover() {
    let mut vp = square();
    let (f, e) = (face(&vp), edge(&vp));
    vp.scene.selection.insert(f);
    vp.set_hover(Some(e)); // cursor elsewhere
    assert!(delete_selection_or_hover(&mut vp));
    assert!(!exists(&vp, f));
    assert!(exists(&vp, e)); // hover untouched
}

#[test]
fn test_nothing_hovered_nothing_selected_is_a_no_op() {
    let mut vp = square();
    assert!(!delete_selection_or_hover(&mut vp));
    assert_eq!(vp.scene.mesh().faces.len(), 1);
    assert!(SelectTool::new().on_key(&mut vp, Key::Delete, Modifiers::NONE));
}

#[test]
fn test_backspace_never_erases_the_hover() {
    let mut vp = square();
    let f = face(&vp);
    vp.set_hover(Some(f));
    assert!(SelectTool::new().on_key(&mut vp, Key::Backspace, Modifiers::NONE));
    assert!(exists(&vp, f));
    assert_eq!(vp.hover_entity(), Some(f));
}

#[test]
fn test_backspace_still_erases_the_selection() {
    let mut vp = square();
    let f = face(&vp);
    vp.scene.selection.insert(f);
    assert!(SelectTool::new().on_key(&mut vp, Key::Backspace, Modifiers::NONE));
    assert!(!exists(&vp, f));
}

#[test]
fn test_other_tools_wait_while_an_operation_holds_geometry() {
    assert!(!SelectTool::new().busy_for_delete()); // idle
    let mut offsetting = OffsetTool::new();
    offsetting.dragging = true; // pushing / dragging
    assert!(offsetting.busy_for_delete());
    let mut held = OffsetTool::new();
    held.base_face = Some((MeshId::LOOSE, square().scene.mesh().face_ids()[0]));
    assert!(held.busy_for_delete());
}
