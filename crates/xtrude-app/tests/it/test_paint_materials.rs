// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Painting with material identity, the eyedropper, restamping a material.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DMat4;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::paint::{paint_state, set_paint_state, PaintState, PaintTool};
use xtrude_app::tools::{paint_sample_armed, set_paint_sample_armed, Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, Viewport, ViewportEvent};
use xtrude_core::attrs::Paint;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{RestampMaterialCommand, SetGroupMaterialCommand};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::materials::Material;

fn quad(m: &mut Mesh, z: f64) -> FaceId {
    m.add_face(
        &[v3(0., 0., z), v3(4., 0., z), v3(4., 4., z), v3(0., 4., z)],
        &[],
    )
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

fn reset() {
    set_paint_state(PaintState::default());
    set_paint_sample_armed(false);
}

fn attrs_mat(vp: &Viewport, f: FaceId) -> Option<String> {
    vp.scene.mesh().face(f).attrs.paint.mat.clone()
}

#[test]
fn test_paint_named_material_stamps_and_registers() {
    reset();
    let mut vp = stub();
    let f = quad(vp.scene.mesh_mut(), 0.0);
    set_paint_state(PaintState {
        color: [0.62, 0.60, 0.58],
        named_color: Some(Material::new("Matte grey", Some([0.62, 0.60, 0.58]), None)),
        ..PaintState::default()
    });
    click(&mut vp, f, Modifiers::NONE);
    assert_eq!(attrs_mat(&vp, f).as_deref(), Some("Matte grey"));
    assert_eq!(
        vp.scene.mesh().face(f).attrs.paint.color,
        Some([0.62, 0.60, 0.58])
    );
    assert!(vp.scene.doc.materials.contains_key("Matte grey"));
    assert!(vp.undo());
    assert!(attrs_mat(&vp, f).is_none());
    assert!(!vp.scene.doc.materials.contains_key("Matte grey"));
    reset();
}

#[test]
fn test_anonymous_paint_clears_identity() {
    reset();
    let mut vp = stub();
    let f = quad(vp.scene.mesh_mut(), 0.0);
    let mat = Material::new("Matte grey", Some([0.62, 0.60, 0.58]), None);
    vp.scene
        .doc
        .materials
        .insert("Matte grey".into(), mat.clone());
    vp.scene.mesh_mut().face_mut(f).attrs.paint = mat.face_paint();
    set_paint_state(PaintState {
        color: [0.8, 0.1, 0.1],
        named_color: None,
        ..PaintState::default()
    });
    click(&mut vp, f, Modifiers::NONE);
    assert!(attrs_mat(&vp, f).is_none());
    assert_eq!(
        vp.scene.mesh().face(f).attrs.paint.color,
        Some([0.8, 0.1, 0.1])
    );
    assert!(vp.scene.doc.materials.contains_key("Matte grey"));
    vp.undo();
    assert_eq!(attrs_mat(&vp, f).as_deref(), Some("Matte grey"));
    reset();
}

#[test]
fn test_eyedropper_picks_up_identity() {
    reset();
    let mut vp = stub();
    let mat = Material::new("Brick red", Some([0.7, 0.3, 0.2]), None);
    vp.scene
        .doc
        .materials
        .insert("Brick red".into(), mat.clone());
    let face = quad(vp.scene.mesh_mut(), 0.0);
    vp.scene.mesh_mut().face_mut(face).attrs.paint = mat.face_paint();
    let anon = quad(vp.scene.mesh_mut(), 1.0);
    vp.scene.mesh_mut().face_mut(anon).attrs.paint.color = Some([0.1, 0.2, 0.3]);
    click(&mut vp, face, Modifiers::ALT);
    assert_eq!(paint_state().named_color, Some(mat));
    click(&mut vp, anon, Modifiers::ALT);
    assert!(paint_state().named_color.is_none());
    reset();
}

#[test]
fn test_restamp_updates_registry_and_every_face() {
    let mut vp = stub();
    let old = Material::new("Matte grey", Some([0.62, 0.60, 0.58]), None);
    vp.scene
        .doc
        .materials
        .insert("Matte grey".into(), old.clone());
    let worn = [
        quad(vp.scene.mesh_mut(), 0.0),
        quad(vp.scene.mesh_mut(), 1.0),
    ];
    for &f in &worn {
        vp.scene.mesh_mut().face_mut(f).attrs.paint = old.face_paint();
    }
    let mut gm = Mesh::new();
    let inner = quad(&mut gm, 5.0);
    gm.face_mut(inner).attrs.paint = old.face_paint();
    let gmid = add_mesh(&mut vp.scene.doc.meshes, gm);
    vp.scene
        .doc
        .groups
        .push(Group::new(gmid, Some("Enclosure")));
    let other = quad(vp.scene.mesh_mut(), 2.0);
    vp.scene.mesh_mut().face_mut(other).attrs.paint.color = Some([0.1, 0.1, 0.1]);
    vp.run(RestampMaterialCommand {
        name: "Matte grey".into(),
        new: Material::new("Matte grey", Some([0.3, 0.5, 0.7]), None),
    });
    assert_eq!(
        vp.scene.doc.materials["Matte grey"].color,
        Some([0.3, 0.5, 0.7])
    );
    for &f in &worn {
        assert_eq!(
            vp.scene.mesh().face(f).attrs.paint.color,
            Some([0.3, 0.5, 0.7])
        );
    }
    let gm = vp.scene.doc.groups[0].mesh;
    assert_eq!(
        vp.scene.doc.mesh(gm).face(inner).attrs.paint.color,
        Some([0.3, 0.5, 0.7])
    );
    assert_eq!(
        vp.scene.mesh().face(other).attrs.paint.color,
        Some([0.1, 0.1, 0.1])
    );
    vp.undo();
    assert_eq!(vp.scene.doc.materials["Matte grey"], old);
    for &f in &worn {
        assert_eq!(
            vp.scene.mesh().face(f).attrs.paint.color,
            Some([0.62, 0.60, 0.58])
        );
    }
    let gm = vp.scene.doc.groups[0].mesh;
    assert_eq!(
        vp.scene.doc.mesh(gm).face(inner).attrs.paint.color,
        Some([0.62, 0.60, 0.58])
    );
}

fn quad_group(vp: &mut Viewport, z: f64, name: &str) -> Group {
    let mut m = Mesh::new();
    quad(&mut m, z);
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    Group::new(id, Some(name))
}

fn material_of(vp: &Viewport, g: GroupId) -> Option<Paint> {
    vp.scene.doc.find_group(g).and_then(|g| g.material.clone())
}

#[test]
fn test_restamp_reaches_groups_painted_whole() {
    let mut vp = stub();
    let old = Material::new("Blue", Some([0.1, 0.6, 0.9]), None);
    vp.scene.doc.materials.insert("Blue".into(), old.clone());
    let face = quad(vp.scene.mesh_mut(), 0.0);
    vp.scene.mesh_mut().face_mut(face).attrs.paint = old.face_paint();
    let mut bx = quad_group(&mut vp, 5.0, "Box");
    let inner = quad_group(&mut vp, 6.0, "Tapa");
    let inner_id = inner.id;
    bx.adopt(vec![inner]);
    let box_id = bx.id;
    vp.scene.doc.groups.push(bx);
    let other = quad_group(&mut vp, 7.0, "Other");
    let other_id = other.id;
    vp.scene.doc.groups.push(other);
    vp.run(SetGroupMaterialCommand {
        group: box_id,
        material: Some(old.face_paint()),
    });
    vp.run(SetGroupMaterialCommand {
        group: other_id,
        material: Some(Paint {
            color: Some([0.2, 0.2, 0.2]),
            mat: None,
            opacity: None,
        }),
    });
    vp.run(SetGroupMaterialCommand {
        group: inner_id,
        material: Some(old.face_paint()),
    });
    vp.run(RestampMaterialCommand {
        name: "Blue".into(),
        new: Material::new("Blue", Some([0.6, 1.0, 0.4]), None),
    });
    assert_eq!(
        vp.scene.mesh().face(face).attrs.paint.color,
        Some([0.6, 1.0, 0.4])
    );
    assert_eq!(
        material_of(&vp, box_id).unwrap().color,
        Some([0.6, 1.0, 0.4])
    );
    assert_eq!(
        material_of(&vp, inner_id).unwrap().color,
        Some([0.6, 1.0, 0.4])
    );
    assert_eq!(
        material_of(&vp, other_id).unwrap().color,
        Some([0.2, 0.2, 0.2])
    );
    vp.undo();
    assert_eq!(
        material_of(&vp, box_id).unwrap().color,
        Some([0.1, 0.6, 0.9])
    );
    assert_eq!(
        material_of(&vp, inner_id).unwrap().color,
        Some([0.1, 0.6, 0.9])
    );
}

#[test]
fn test_the_eyedropper_button_arms_one_sample_and_pops_out() {
    reset();
    let mut vp = stub();
    let src = quad(vp.scene.mesh_mut(), 0.0);
    vp.scene.mesh_mut().face_mut(src).attrs.paint.color = Some([0.2, 0.4, 0.6]);
    let target = quad(vp.scene.mesh_mut(), 3.0);
    vp.scene.mesh_mut().face_mut(target).attrs.paint.color = Some([0.9, 0.9, 0.9]);
    set_paint_sample_armed(true);
    vp.take_events();
    click(&mut vp, src, Modifiers::NONE);
    assert_eq!(paint_state().color, [0.2, 0.4, 0.6]);
    assert!(!paint_sample_armed());
    assert!(vp.take_events().contains(&ViewportEvent::PaintSampled));
    click(&mut vp, target, Modifiers::NONE);
    assert_eq!(
        vp.scene.mesh().face(target).attrs.paint.color,
        Some([0.2, 0.4, 0.6])
    );
    reset();
}

#[test]
fn test_restamping_a_material_reaches_faces_inside_components() {
    let mut vp = stub();
    let mut deep = Mesh::new();
    let inner = deep.add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    deep.face_mut(inner).attrs.paint = Paint {
        color: Some([0.5, 0.5, 0.5]),
        mat: Some("Slate grey".into()),
        opacity: None,
    };
    let hid = add_mesh(&mut vp.scene.doc.meshes, deep);
    let mut child = Group::new(hid, Some("Clip"));
    child.xform = Some(DMat4::IDENTITY);
    let empty = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut parent = Group::new(empty, Some("Frame"));
    parent.adopt(vec![child]);
    vp.scene.doc.groups.push(parent);
    let loose = vp.scene.mesh_mut().add_face(
        &[
            v3(9., 0., 0.),
            v3(11., 0., 0.),
            v3(11., 2., 0.),
            v3(9., 2., 0.),
        ],
        &[],
    );
    vp.scene.mesh_mut().face_mut(loose).attrs.paint = Paint {
        color: Some([0.5, 0.5, 0.5]),
        mat: Some("Slate grey".into()),
        opacity: None,
    };
    vp.scene.doc.materials.insert(
        "Slate grey".into(),
        Material::new("Slate grey", Some([0.5, 0.5, 0.5]), None),
    );
    vp.run(RestampMaterialCommand {
        name: "Slate grey".into(),
        new: Material::new("Slate grey", Some([0.9, 0.2, 0.1]), None),
    });
    assert_eq!(
        vp.scene.mesh().face(loose).attrs.paint.color,
        Some([0.9, 0.2, 0.1])
    );
    let child_mesh = vp.scene.doc.groups[0].children[0].mesh;
    assert_eq!(
        vp.scene.doc.mesh(child_mesh).face(inner).attrs.paint.color,
        Some([0.9, 0.2, 0.1]),
        "nested face missed"
    );
}
