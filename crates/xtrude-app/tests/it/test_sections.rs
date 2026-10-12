// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Section planes: the tool tests (entity, commands and persistence live in
//! crates/xtrude-io).

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::section::SectionPlaneTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, Viewport};
use xtrude_model::ids::{MeshId, ObjId};
use xtrude_model::section::SectionPlane;

fn ctx(x: f64, y: f64, z: f64) -> ToolContext {
    ToolContext::at(v3(x, y, z))
}

fn plane(vp: &Viewport, id: ObjId) -> &SectionPlane {
    vp.scene
        .doc
        .section_planes
        .iter()
        .find(|p| p.id == id)
        .unwrap()
}

#[test]
fn test_tool_aligns_to_hovered_face_and_places_active() {
    let mut vp = stub();
    let wall = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 0., 2.),
            v3(0., 0., 2.),
        ],
        &[],
    ); // a Y-normal wall
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: wall,
        placement: None,
        xform: None,
        owner: None,
    }));
    let mut t = SectionPlaneTool::new();
    t.on_activate(&mut vp);
    t.on_hover(&mut vp, &ctx(1., 0., 1.));
    t.on_click(&mut vp, &ctx(1., 0., 1.));
    assert_eq!(vp.scene.doc.section_planes.len(), 1);
    let sp = &vp.scene.doc.section_planes[0];
    assert!(sp.active); // new plane = the active cut
    assert!((sp.normal.y.abs() - 1.0).abs() < 1e-6); // aligned to the wall
    assert_eq!(sp.symbol, "A"); // sections are named by LETTER
    assert!(vp.undo());
    assert!(vp.scene.doc.section_planes.is_empty());
}

#[test]
fn test_tool_arrow_keys_and_ground_default() {
    let mut vp = stub();
    let mut t = SectionPlaneTool::new();
    t.on_activate(&mut vp);
    t.on_hover(&mut vp, &ctx(0., 0., 0.)); // empty ground → horizontal cut
    assert!((t.current_normal().z - 1.0).abs() < 1e-9);
    assert!(t.on_key(&mut vp, Key::Right, Modifiers::NONE)); // red = X
    assert!((t.current_normal().x - 1.0).abs() < 1e-9);
    assert!(t.on_key(&mut vp, Key::Left, Modifiers::NONE)); // green = Y
    assert!((t.current_normal().y - 1.0).abs() < 1e-9);
    assert!(t.on_key(&mut vp, Key::Up, Modifiers::NONE)); // blue = Z
    assert!((t.current_normal().z - 1.0).abs() < 1e-9);
    assert!(t.on_key(&mut vp, Key::Down, Modifiers::NONE)); // back to face
    assert!(t.axis_pick.is_none());
}

fn grabbing(vp: &mut Viewport, sp: SectionPlane) -> ObjId {
    let id = sp.id;
    vp.scene.doc.section_planes.push(sp);
    FakeProbe::of(vp).pick_section_plane(move |_, _| Some(id));
    FakeProbe::of(vp).pick_group(|_, _| None);
    FakeProbe::of(vp).pick_edge(|_, _| None);
    FakeProbe::of(vp).pick_text_label(|_, _, _| None);
    id
}

#[test]
fn test_move_grabs_a_section_plane_directly() {
    // No pre-selection — Move grabs the plane by its frame and the cut
    // follows live; the drop is one undoable command.
    let mut vp = stub();
    let id = grabbing(
        &mut vp,
        SectionPlane::new(v3(0., 0., 1.), v3(0., 0., 1.), "", "", false, None),
    );
    vp.scene.set_active_section(Some(id));
    let mut t = MoveTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0., 1.)); // grab the plane directly
    assert_eq!(t.splanes, vec![id]);
    t.on_hover(&mut vp, &ctx(0., 0., 3.)); // live: the cut moves with it
    assert!((plane(&vp, id).point.z - 3.0).abs() < 1e-9);
    t.on_click(&mut vp, &ctx(0., 0., 3.)); // drop
    assert!((plane(&vp, id).point.z - 3.0).abs() < 1e-9);
    assert!(vp.undo());
    assert!((plane(&vp, id).point.z - 1.0).abs() < 1e-9);
}

#[test]
fn test_rotate_grabs_a_section_plane_directly() {
    let mut vp = stub();
    let id = grabbing(
        &mut vp,
        SectionPlane::new(v3(0., 0., 0.), v3(0., 0., 1.), "", "", false, None),
    );
    let mut t = RotateTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0., 0.)); // centre grabs the plane
    assert_eq!(t.splanes, vec![id]);
    t.on_click(&mut vp, &ctx(1., 0., 0.)); // base arm
    t.on_hover(&mut vp, &ctx(0., 1., 0.));
    t.on_click(&mut vp, &ctx(0., 1., 0.)); // +90° about Z
    assert_eq!(vp.history.undo_len(), 1);
    assert!(vp.undo());
}

#[test]
fn test_axis_locked_and_ground_planes_hide_the_camera_side() {
    let mut vp = stub();
    let mut t = SectionPlaneTool::new();
    t.on_activate(&mut vp);
    FakeProbe::of(&mut vp).eye(Some(v3(0., -30., 10.))); // looking north, from above
    t.on_hover(&mut vp, &ctx(0., -5., 0.)); // beside the model, on the ground
    assert!(t.on_key(&mut vp, Key::Left, Modifiers::NONE)); // green = Y
    let n = t.current_normal();
    assert!((n.y + 1.0).abs() < 1e-9); // toward the camera (south)
    t.on_click(&mut vp, &ctx(0., -5., 0.));
    let sp = vp.scene.doc.section_planes.last().unwrap();
    assert!(sp.side(v3(0., -30., 10.)) > 0.0); // the eye's side is hidden
    assert!(sp.side(v3(0., 0., 1.)) < 0.0); // the model stays
                                            // camera north of the spot: the same lock now points north
    FakeProbe::of(&mut vp).eye(Some(v3(0., 30., 10.)));
    t.on_hover(&mut vp, &ctx(0., -5., 0.));
    assert!((t.current_normal().y - 1.0).abs() < 1e-9);
    // the ground default faces the camera too: from below it points down
    assert!(t.on_key(&mut vp, Key::Down, Modifiers::NONE));
    FakeProbe::of(&mut vp).eye(Some(v3(0., 0., -10.)));
    t.on_hover(&mut vp, &ctx(0., -5., 0.));
    assert!((t.current_normal().z + 1.0).abs() < 1e-9);
    // no camera at all (headless stand-ins): the raw axis is kept
    FakeProbe::of(&mut vp).eye(None);
    assert!(t.on_key(&mut vp, Key::Up, Modifiers::NONE));
    assert!((t.current_normal().z - 1.0).abs() < 1e-9);
}
