// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Paint tool tests: paint clicked on a group from outside paints the
//! group, the eyedropper on a default face inside a painted group samples the
//! group's paint; and the bake, read back from the frame cache
//! (`Viewport::frame_data`).

use crate::common;
use crate::common::FakeProbe;

use common::{rgb_key, scene_triangles, stub, v3};
use glam::{DMat4, DVec3};
use xtrude_app::input::Modifiers;
use xtrude_app::tools::paint::{paint_state, set_paint_state, PaintState, PaintTool};
use xtrude_app::tools::{set_paint_sample_armed, Tool, ToolContext};
use xtrude_app::viewport::render::shaded_color;
use xtrude_app::viewport::{FaceHit, Viewport};
use xtrude_core::attrs::Paint;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::SetGroupMaterialCommand;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

fn two_face_group(vp: &mut Viewport) -> (GroupId, FaceId, FaceId) {
    let mut m = Mesh::new();
    let plain = m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let blue = m.add_face(
        &[
            v3(2., 0., 0.),
            v3(3., 0., 0.),
            v3(3., 1., 0.),
            v3(2., 1., 0.),
        ],
        &[],
    );
    m.face_mut(blue).attrs.paint.color = Some([0.0, 0.0, 1.0]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("box"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    (gid, plain, blue)
}

/// The face under the cursor belongs to a group picked from outside.
fn aim(vp: &mut Viewport, g: GroupId, face: FaceId) {
    let mesh = vp.scene.doc.find_group(g).unwrap().mesh;
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh,
        face,
        placement: Some(g),
        xform: None,
        owner: Some(g),
    }));
    FakeProbe::of(vp).pick_group(move |_, _| Some(g));
}

fn click(vp: &mut Viewport, mods: Modifiers) {
    PaintTool::new().on_click(vp, &ToolContext::at(v3(0., 0., 0.)).with_modifiers(mods));
}

#[test]
fn test_paint_clicked_on_a_group_from_outside_paints_the_group() {
    set_paint_sample_armed(false);
    let mut vp = stub();
    let (g, plain, blue) = two_face_group(&mut vp);
    aim(&mut vp, g, plain);
    set_paint_state(PaintState {
        color: [1.0, 0.0, 0.0],
        ..PaintState::default()
    });
    click(&mut vp, Modifiers::NONE);
    let grp = vp.scene.doc.find_group(g).unwrap();
    assert_eq!(
        grp.material,
        Some(Paint {
            color: Some([1.0, 0.0, 0.0]),
            ..Paint::default()
        })
    );
    let m = vp.scene.doc.mesh(grp.mesh);
    assert!(m.face(plain).attrs.paint.color.is_none()); // the face itself untouched
    assert_eq!(m.face(blue).attrs.paint.color, Some([0.0, 0.0, 1.0]));
    assert!(vp.undo());
    assert!(vp.scene.doc.find_group(g).unwrap().material.is_none());
    set_paint_state(PaintState::default());
}

#[test]
fn test_the_eyedropper_on_a_default_face_inside_a_painted_group_samples_the_groups_paint() {
    set_paint_sample_armed(false);
    let mut vp = stub();
    let (g, plain, _) = two_face_group(&mut vp);
    vp.scene.doc.find_group_mut(g).unwrap().material = Some(Paint {
        color: Some([0.2, 0.4, 0.6]),
        ..Paint::default()
    });
    aim(&mut vp, g, plain);
    set_paint_state(PaintState {
        color: [1.0, 1.0, 1.0],
        ..PaintState::default()
    });
    click(&mut vp, Modifiers::ALT);
    let c = paint_state().color.map(|v| (v * 1000.0).round() / 1000.0);
    assert_eq!(c, [0.2, 0.4, 0.6]);
    set_paint_state(PaintState::default());
}

// ---- the bake: what the viewport actually draws ---------------------------

const RED: [f64; 3] = [1.0, 0.0, 0.0];

fn red_paint() -> Paint {
    Paint {
        color: Some(RED),
        ..Paint::default()
    }
}

/// `shaded_color` on a +Z face, as the frame stores it.
fn shaded(c: [f64; 3]) -> [i32; 3] {
    let [r, g, b] = shaded_color(c, DVec3::Z);
    rgb_key([r as f32, g as f32, b as f32, 1.0])
}

/// The front colours the frame draws, of the corners whose x is in `xs`.
fn colours(vp: &mut Viewport, xs: std::ops::Range<f64>) -> Vec<[i32; 3]> {
    scene_triangles(vp)
        .iter()
        .flatten()
        .filter(|c| xs.contains(&c.pos.x))
        .map(|c| rgb_key(c.front))
        .collect()
}

fn everywhere() -> std::ops::Range<f64> {
    f64::MIN..f64::MAX
}

#[test]
fn test_the_bake_dresses_default_faces_and_keeps_painted_ones() {
    let mut vp = stub();
    let (g, _plain, _blue) = two_face_group(&mut vp);
    let (red, blue) = (shaded(RED), shaded([0.0, 0.0, 1.0]));
    let plain = colours(&mut vp, everywhere());
    assert!(plain.contains(&blue) && !plain.contains(&red));
    vp.run(SetGroupMaterialCommand {
        group: g,
        material: Some(red_paint()),
    });
    let painted = colours(&mut vp, everywhere());
    assert!(painted.contains(&red) && painted.contains(&blue)); // rule: own wins
    vp.run(SetGroupMaterialCommand {
        group: g,
        material: None,
    });
    assert!(!colours(&mut vp, everywhere()).contains(&red)); // unpaint rebakes
}

/// Two instances of one unit square, at x = 0 and x = 3.
fn two_instances(vp: &mut Viewport) -> (GroupId, GroupId) {
    let mut proto = Mesh::new();
    proto.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let mut ids = Vec::new();
    for (name, dx) in [("a", 0.0), ("b", 3.0)] {
        let mut g = Group::new(mid, Some(name));
        g.xform = Some(DMat4::from_translation(DVec3::new(dx, 0., 0.)));
        ids.push(g.id);
        vp.scene.doc.groups.push(g);
    }
    vp.scene.version += 1;
    (ids[0], ids[1])
}

#[test]
fn test_two_instances_of_one_prototype_can_wear_different_paint() {
    let mut vp = stub();
    let (_a, b) = two_instances(&mut vp);
    vp.scene.doc.find_group_mut(b).unwrap().material = Some(red_paint());
    vp.scene.version += 1;
    let red = shaded(RED);
    assert!(!colours(&mut vp, -0.5..1.5).contains(&red));
    assert!(colours(&mut vp, 2.5..4.5).contains(&red));
}

#[test]
fn test_the_instanced_pool_splits_a_painted_instance_from_its_siblings() {
    // An instance painted from outside must change colour. Each placement is
    // baked, so the contract checked is the drawn result after a paint of an
    // already-drawn sibling.
    let mut vp = stub();
    let (_a, b) = two_instances(&mut vp);
    let red = shaded(RED);
    assert!(!colours(&mut vp, everywhere()).contains(&red));
    vp.run(SetGroupMaterialCommand {
        group: b,
        material: Some(red_paint()),
    });
    assert!(!colours(&mut vp, -0.5..1.5).contains(&red));
    assert!(colours(&mut vp, 2.5..4.5).contains(&red));
}
