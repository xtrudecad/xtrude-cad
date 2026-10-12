// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Materials: the command and Paint tool tests (the .igz ones live in
//! crates/xtrude-io/tests/it/test_materials.rs).

use crate::common;
use crate::common::FakeProbe;

use common::{push, stub, up, v3};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::paint::{paint_state, set_paint_state, PaintState, PaintTool};
use xtrude_app::tools::{set_paint_sample_armed, Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, Viewport};
use xtrude_core::FaceId;
use xtrude_model::commands::{AddFaceCommand, SetFaceColorCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::Command;
use xtrude_model::ids::MeshId;

fn square(vp: &mut Viewport, x0: f64) -> FaceId {
    let before = vp.scene.mesh().face_ids();
    let lp = vec![
        v3(x0, 0., 0.),
        v3(x0 + 4., 0., 0.),
        v3(x0 + 4., 4., 0.),
        v3(x0, 4., 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (lp[i], lp[(i + 1) % 4])).collect();
    let face: Box<dyn Command> = Box::new(AddFaceCommand::new(lp));
    let cmd = build_add_edges(&vp.scene, &segs, false, vec![face], None);
    vp.execute_boxed(cmd);
    vp.scene
        .mesh()
        .face_ids()
        .into_iter()
        .find(|f| !before.contains(f))
        .expect("the square's face")
}

fn color(vp: &Viewport, f: FaceId) -> Option<[f64; 3]> {
    vp.scene.mesh().face(f).attrs.paint.color
}

fn with_color(rgb: [f64; 3]) {
    set_paint_sample_armed(false);
    set_paint_state(PaintState {
        color: rgb,
        ..PaintState::default()
    });
}

fn click(vp: &mut Viewport, face: FaceId, mods: Modifiers) {
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face,
        placement: None,
        xform: None,
        owner: None,
    }));
    let mut t = PaintTool::new();
    t.on_click(vp, &ToolContext::at(v3(0., 0., 0.)).with_modifiers(mods));
}

#[test]
fn test_set_face_color_command_do_undo() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    assert!(color(&vp, f).is_none());
    vp.run(SetFaceColorCommand {
        faces: vec![(MeshId::LOOSE, f)],
        color: Some([0.2, 0.4, 0.8]),
    });
    assert_eq!(color(&vp, f), Some([0.2, 0.4, 0.8]));
    assert!(vp.undo());
    assert!(color(&vp, f).is_none());
    assert!(vp.redo());
    assert_eq!(color(&vp, f), Some([0.2, 0.4, 0.8]));
}

#[test]
fn test_set_face_color_none_clears() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.1, 0.1, 0.1]);
    vp.run(SetFaceColorCommand {
        faces: vec![(MeshId::LOOSE, f)],
        color: None,
    });
    assert!(color(&vp, f).is_none());
    assert!(vp.undo());
    assert_eq!(color(&vp, f), Some([0.1, 0.1, 0.1]));
}

#[test]
fn test_paint_tool_colours_clicked_face() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    with_color([0.7, 0.3, 0.1]);
    click(&mut vp, f, Modifiers::NONE);
    assert_eq!(color(&vp, f), Some([0.7, 0.3, 0.1]));
    assert!(vp.undo());
    assert!(color(&vp, f).is_none());
    set_paint_state(PaintState::default());
}

#[test]
fn test_paint_tool_alt_samples_colour() {
    let mut vp = stub();
    let f = square(&mut vp, 0.);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.11, 0.22, 0.33]);
    with_color([0.9, 0.9, 0.9]);
    let steps = vp.history.undo_len();
    click(&mut vp, f, Modifiers::ALT);
    assert_eq!(paint_state().color, [0.11, 0.22, 0.33]);
    // Sampling does not record an undo step (no painting happened).
    assert_eq!(vp.history.undo_len(), steps);
    set_paint_state(PaintState::default());
}

#[test]
fn test_paint_tool_paints_whole_face_selection() {
    let mut vp = stub();
    square(&mut vp, 0.);
    square(&mut vp, 4.);
    // The shared edge rebuilds the first square's face, so find both faces
    // afresh.
    let m = vp.scene.mesh();
    let left = |f: &FaceId| m.face_positions(*f).iter().all(|p| p.x <= 4.0 + 1e-9);
    let (a, b) = {
        let ids = m.face_ids();
        let a = *ids.iter().find(|f| left(f)).unwrap();
        let b = *ids.iter().find(|f| !left(f)).unwrap();
        (a, b)
    };
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, a));
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, b));
    with_color([0.5, 0.5, 0.0]);
    click(&mut vp, a, Modifiers::NONE);
    assert_eq!(color(&vp, a), Some([0.5, 0.5, 0.0]));
    assert_eq!(color(&vp, b), Some([0.5, 0.5, 0.0]));
    set_paint_state(PaintState::default());
}

#[test]
fn test_color_survives_pushpull() {
    let mut vp = stub();
    let base = square(&mut vp, 0.);
    let d = up(&vp, base, 3.0);
    assert!(push(&mut vp, base, d, false));
    let at_z = |vp: &Viewport, z: f64| {
        let m = vp.scene.mesh();
        m.face_ids()
            .into_iter()
            .find(|&f| m.face_positions(f).iter().all(|p| (p.z - z).abs() < 1e-9))
            .expect("a face at that height")
    };
    let top = at_z(&vp, 3.0);
    vp.run(SetFaceColorCommand {
        faces: vec![(MeshId::LOOSE, top)],
        color: Some([0.9, 0.1, 0.1]),
    });
    assert!(push(&mut vp, top, 2.0, false));
    let new_top = at_z(&vp, 5.0);
    assert_eq!(color(&vp, new_top), Some([0.9, 0.1, 0.1]));
}
