// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shift toggles, Ctrl adds, Shift+Ctrl removes, a plain click replaces; the
//! box reads the same; a modified click on empty space keeps the selection.

use crate::common;
use crate::common::FakeProbe;

use std::collections::BTreeSet;

use common::{stub, v3};
use xtrude_app::input::Modifiers;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{selection_mode, Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, PickedEdge, Viewport};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

fn plan() -> (Viewport, EntityRef, EntityRef, EntityRef) {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x, p.y)));
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    let m = vp.scene.mesh_mut();
    let a = m.add_edge(v3(1., 1., 0.), v3(2., 2., 0.)).unwrap();
    let b = m.add_edge(v3(4., 4., 0.), v3(10., 10., 0.)).unwrap();
    let f = m.add_face(
        &[
            v3(1., 1., 0.),
            v3(2., 1., 0.),
            v3(2., 2., 0.),
            v3(1., 2., 0.),
        ],
        &[],
    );
    (
        vp,
        EntityRef::Edge(MeshId::LOOSE, a),
        EntityRef::Edge(MeshId::LOOSE, b),
        EntityRef::Face(MeshId::LOOSE, f),
    )
}

/// The viewport picks what the test hands it.
fn click(vp: &mut Viewport, what: Option<EntityRef>, mods: Modifiers) {
    let edge = match what {
        Some(EntityRef::Edge(m, e)) => {
            let (a, b) = vp.scene.doc.mesh(m).edge_ab(e);
            Some(PickedEdge {
                a,
                b,
                edge: Some((m, e)),
                in_group: false,
                context: None,
                group: None,
                soft: false,
                curve: None,
            })
        }
        _ => None,
    };
    FakeProbe::of(vp).pick_edge(move |_, _| edge.clone());
    FakeProbe::of(vp).pick(match what {
        Some(EntityRef::Face(m, f)) => Some(FaceHit {
            mesh: m,
            face: f,
            placement: None,
            xform: None,
            owner: None,
        }),
        _ => None,
    });
    SelectTool::new().on_click(vp, &ToolContext::at(v3(0., 0., 0.)).with_modifiers(mods));
}

fn sel(vp: &Viewport) -> BTreeSet<EntityRef> {
    vp.scene.selection.iter().copied().collect()
}

fn set(e: &[EntityRef]) -> BTreeSet<EntityRef> {
    e.iter().copied().collect()
}

const SHIFT_CTRL: Modifiers = Modifiers {
    shift: true,
    ctrl: true,
    alt: false,
    meta: false,
};

#[test]
fn test_the_modifiers_map_to_the_four_modes() {
    assert_eq!(selection_mode(Modifiers::NONE), SelectMode::Replace);
    assert_eq!(selection_mode(Modifiers::SHIFT), SelectMode::Toggle);
    assert_eq!(selection_mode(Modifiers::CTRL), SelectMode::Add);
    assert_eq!(selection_mode(SHIFT_CTRL), SelectMode::Remove);
}

#[test]
fn test_shift_click_takes_a_selected_edge_out_and_puts_an_unselected_one_in() {
    let (mut vp, a, b, _) = plan();
    click(&mut vp, Some(a), Modifiers::NONE);
    assert_eq!(sel(&vp), set(&[a]));
    click(&mut vp, Some(b), Modifiers::SHIFT);
    assert_eq!(sel(&vp), set(&[a, b]));
    click(&mut vp, Some(a), Modifiers::SHIFT);
    assert_eq!(sel(&vp), set(&[b]));
}

#[test]
fn test_ctrl_adds_and_shift_ctrl_removes() {
    let (mut vp, a, b, _) = plan();
    click(&mut vp, Some(a), Modifiers::NONE);
    click(&mut vp, Some(b), Modifiers::CTRL);
    assert_eq!(sel(&vp), set(&[a, b]));
    click(&mut vp, Some(b), Modifiers::CTRL); // Ctrl never takes away
    assert_eq!(sel(&vp), set(&[a, b]));
    click(&mut vp, Some(b), SHIFT_CTRL);
    assert_eq!(sel(&vp), set(&[a]));
    click(&mut vp, Some(b), SHIFT_CTRL); // already out: no-op
    assert_eq!(sel(&vp), set(&[a]));
}

#[test]
fn test_a_plain_click_still_replaces_the_whole_selection() {
    let (mut vp, a, b, _) = plan();
    vp.scene.select([a, b], SelectMode::Replace);
    click(&mut vp, Some(a), Modifiers::NONE);
    assert_eq!(sel(&vp), set(&[a]));
}

#[test]
fn test_a_modified_click_on_empty_space_keeps_the_selection() {
    let (mut vp, a, _, _) = plan();
    vp.scene.select([a], SelectMode::Replace);
    click(&mut vp, None, Modifiers::SHIFT); // missed while carving
    assert_eq!(sel(&vp), set(&[a]));
    click(&mut vp, None, Modifiers::CTRL);
    assert_eq!(sel(&vp), set(&[a]));
    click(&mut vp, None, Modifiers::NONE); // a bare click does clear
    assert!(vp.scene.selection.is_empty());
}

#[test]
fn test_a_face_pick_takes_its_whole_surface_out() {
    let (mut vp, _, _, f) = plan();
    click(&mut vp, Some(f), Modifiers::NONE);
    assert!(vp.scene.selection.contains(&f));
    click(&mut vp, Some(f), Modifiers::SHIFT);
    assert!(!vp.scene.selection.contains(&f));
}

#[test]
fn test_the_box_toggles_with_shift_and_removes_with_shift_ctrl() {
    let (mut vp, inside, outside, _) = plan();
    let rect = (0.0, 0.0, 5.0, 5.0);
    vp.scene.select([inside, outside], SelectMode::Replace);
    SelectTool::new().on_box_select(&mut vp, rect, false, SelectMode::Toggle);
    assert!(!vp.scene.selection.contains(&inside)); // caught → dropped
    assert!(vp.scene.selection.contains(&outside)); // untouched by the box
    SelectTool::new().on_box_select(&mut vp, rect, false, SelectMode::Toggle);
    assert!(vp.scene.selection.contains(&inside)); // back in
    SelectTool::new().on_box_select(&mut vp, rect, false, SelectMode::Remove);
    assert!(!vp.scene.selection.contains(&inside) && vp.scene.selection.contains(&outside));
}

#[test]
fn test_scene_select_modes_are_explicit() {
    let (mut vp, a, b, _) = plan();
    let s = &mut vp.scene;
    s.select([a], SelectMode::Replace);
    s.select([b], SelectMode::Add);
    assert_eq!(
        s.selection.iter().copied().collect::<BTreeSet<_>>(),
        set(&[a, b])
    );
    s.select([a], SelectMode::Remove);
    assert_eq!(
        s.selection.iter().copied().collect::<BTreeSet<_>>(),
        set(&[b])
    );
    s.select([a, b], SelectMode::Toggle);
    assert_eq!(
        s.selection.iter().copied().collect::<BTreeSet<_>>(),
        set(&[a])
    );
}
