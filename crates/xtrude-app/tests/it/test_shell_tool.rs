// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Click a solid, click the faces to leave open, Enter or a typed wall
//! thickness hollows it — one undo step.

use crate::common;

use common::{add_box, sized_viewport, v3, FakeProbe};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::shell::{set_shell_thickness, ShellTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{FaceHit, Viewport, ViewportEvent};
use xtrude_app::window::hints::hint_for;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

const MM: f64 = 0.001;

/// A 20 mm cube.
fn cube(vp: &mut Viewport) {
    add_box(vp, DVec3::ZERO, v3(20., 20., 20.) * MM);
    vp.scene.version += 1;
}

fn face_up(vp: &Viewport) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z > 0.5)
        .unwrap()
}

/// Click `face`, as if it were under the cursor.
fn click(vp: &mut Viewport, face: FaceId) {
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face,
        placement: None,
        xform: None,
        owner: None,
    }));
    let c = ToolContext::at(DVec3::ZERO);
    vp.with_tool(|t, vp| t.on_hover(vp, &c));
    vp.with_tool(|t, vp| t.on_click(vp, &c));
}

fn tool(vp: &Viewport) -> &ShellTool {
    vp.tool::<ShellTool>().expect("the shell tool")
}

fn volume_mm3(vp: &Viewport) -> f64 {
    signed_volume(vp.scene.mesh()) / MM.powi(3)
}

#[test]
fn test_click_the_solid_and_its_top_then_enter_hollows_a_box() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    set_shell_thickness(2. * MM);
    vp.set_active_tool(Some(Box::new(ShellTool::new())));
    let top = face_up(&vp);
    let side = vp.scene.mesh().face_ids()[0];
    click(&mut vp, side);
    assert_eq!(tool(&vp).solid.len(), 6);
    assert!(tool(&vp).open.is_empty());
    assert!(hint_for(Some("shell"), vp.active_tool(), None, None).contains("open"));
    click(&mut vp, top);
    assert_eq!(tool(&vp).open, vec![top]);
    // the rim previews where the top was
    assert_eq!(tool(&vp).preview_faces().len(), 1);
    assert!(!vp
        .with_tool(|t, vp| t.rubber_band_lines(vp))
        .unwrap()
        .is_empty());
    let depth = vp.history.undo_len();
    vp.key_press(&KeyEvent::new(Key::Return, "", Modifiers::NONE));
    assert_eq!(vp.history.undo_len(), depth + 1);
    assert!(vp.scene.mesh().faces.len() == 11 && is_closed(vp.scene.mesh()));
    let v = volume_mm3(&vp);
    assert!((v - (8000. - 16. * 16. * 18.)).abs() < 1e-6, "{v}");
    assert!(tool(&vp).solid.is_empty());
    vp.undo();
    assert_eq!(vp.scene.mesh().faces.len(), 6);
    vp.redo();
    assert_eq!(vp.scene.mesh().faces.len(), 11);
}

#[test]
fn test_a_second_click_closes_a_face_again_and_a_typed_wall_applies() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    vp.set_active_tool(Some(Box::new(ShellTool::new())));
    let top = face_up(&vp);
    click(&mut vp, top);
    click(&mut vp, top);
    click(&mut vp, top);
    assert!(tool(&vp).open.is_empty());
    assert_eq!(
        vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(2. * MM))),
        Some(true)
    );
    // a closed cavity inside
    assert!(vp.scene.mesh().faces.len() == 12 && is_closed(vp.scene.mesh()));
    let v = volume_mm3(&vp);
    assert!((v - (8000. - 16. * 16. * 16.)).abs() < 1e-6, "{v}");
}

#[test]
fn test_selected_faces_start_open() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    let top = face_up(&vp);
    vp.scene
        .select([EntityRef::Face(MeshId::LOOSE, top)], SelectMode::Replace);
    vp.set_active_tool(Some(Box::new(ShellTool::new())));
    assert_eq!(tool(&vp).solid.len(), 6);
    assert_eq!(tool(&vp).open, vec![top]);
    vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(2. * MM)));
    assert_eq!(vp.scene.mesh().faces.len(), 11);
}

#[test]
fn test_a_wall_too_thick_is_refused_and_said() {
    let mut vp = sized_viewport(1000.0, 700.0);
    cube(&mut vp);
    vp.set_active_tool(Some(Box::new(ShellTool::new())));
    let top = face_up(&vp);
    click(&mut vp, top);
    click(&mut vp, top);
    vp.take_events();
    let depth = vp.history.undo_len();
    vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Num(11. * MM)));
    assert_eq!(vp.history.undo_len(), depth);
    assert_eq!(vp.scene.mesh().faces.len(), 6);
    let said: Vec<String> = vp
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect();
    assert!(said.iter().any(|s| s.contains("too thick")), "{said:?}");
    // still picked: a thinner wall can follow
    assert_eq!(tool(&vp).open, vec![top]);
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert!(tool(&vp).solid.is_empty());
}
