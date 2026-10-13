// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The model's colours as filament slots: the 3MF export's
//! `<basematerials>` order, with the face area of each.

use crate::common;
use common::*;
use std::io::Read;
use xtrude_core::attrs::Paint;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_io::threemf::{colour_slots, save_3mf};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

/// A 10 mm cube at the origin, every face wound outward; face area 1e-4 m².
fn cube() -> (Mesh, Vec<xtrude_core::FaceId>) {
    let mut m = Mesh::new();
    let s = 0.01;
    let p = |x: bool, y: bool, z: bool| {
        DVec3::new(
            if x { s } else { 0. },
            if y { s } else { 0. },
            if z { s } else { 0. },
        )
    };
    let (f, t) = (false, true);
    let faces = [
        [p(f, f, f), p(f, t, f), p(t, t, f), p(t, f, f)],
        [p(f, f, t), p(t, f, t), p(t, t, t), p(f, t, t)],
        [p(f, f, f), p(t, f, f), p(t, f, t), p(f, f, t)],
        [p(f, t, f), p(f, t, t), p(t, t, t), p(t, t, f)],
        [p(f, f, f), p(f, f, t), p(f, t, t), p(f, t, f)],
        [p(t, f, f), p(t, t, f), p(t, t, t), p(t, f, t)],
    ]
    .iter()
    .map(|ring| m.add_face(ring, &[]))
    .collect();
    (m, faces)
}

fn paint(rgb: [f64; 3], name: Option<&str>) -> Option<Paint> {
    Some(Paint {
        color: Some(rgb),
        mat: name.map(String::from),
        opacity: None,
    })
}

/// Loose geometry (two red faces, four unpainted), a blue component placed
/// twice (once stretched to twice its length along x), a green group
/// mirrored, and a hidden yellow group.
fn scene() -> Scene {
    let mut s = Scene::new();
    let (mut loose, faces) = cube();
    for &f in &faces[..2] {
        loose.face_mut(f).attrs.paint = paint([1., 0., 0.], Some("Red")).unwrap();
    }
    *s.mesh_mut() = loose;
    let peg = add_mesh(&mut s.doc.meshes, cube().0);
    for m in [
        DMat4::from_translation(DVec3::new(0.05, 0., 0.)),
        DMat4::from_translation(DVec3::new(0.1, 0., 0.))
            * DMat4::from_scale(DVec3::new(2., 1., 1.)),
    ] {
        let mut g = Group::new(peg, Some("Peg"));
        g.xform = Some(m);
        g.material = paint([0., 0., 1.], None);
        s.doc.groups.push(g);
    }
    let mut g = Group::new(add_mesh(&mut s.doc.meshes, cube().0), Some("Mirrored"));
    g.component = false;
    g.xform = Some(DMat4::from_scale(DVec3::new(-1., 1., 1.)));
    g.material = paint([0., 1., 0.], None);
    s.doc.groups.push(g);
    let mut g = Group::new(add_mesh(&mut s.doc.meshes, cube().0), Some("Hidden"));
    g.component = false;
    g.hidden = true;
    g.material = paint([1., 1., 0.], None);
    s.doc.groups.push(g);
    s
}

/// The `displaycolor` of every `<base>` the 3MF export writes.
fn exported_bases(s: &Scene) -> Vec<String> {
    let tmp = TmpDir::new();
    let path = tmp.join("out.3mf");
    save_3mf(s, &path).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut model = String::new();
    z.by_name("3D/3dmodel.model")
        .unwrap()
        .read_to_string(&mut model)
        .unwrap();
    let doc = roxmltree::Document::parse(&model).unwrap();
    doc.descendants()
        .filter(|n| n.has_tag_name("base"))
        .map(|b| b.attribute("displaycolor").unwrap().to_string())
        .collect()
}

fn hex(rgb: [f64; 3]) -> String {
    let b = |v: f64| (v * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(rgb[0]), b(rgb[1]), b(rgb[2]))
}

#[test]
fn test_the_slots_are_the_base_materials_of_the_3mf_export_in_order() {
    let s = scene();
    let slots: Vec<String> = colour_slots(&s).iter().map(|c| hex(c.key.color)).collect();
    assert_eq!(slots, exported_bases(&s));
    // The red faces come first, the unpainted ones in the default
    // colour next, then the groups in their order; the hidden group is
    // not printed, so it takes no slot.
    assert_eq!(slots.len(), 4);
    assert_eq!(slots[0], "#FF0000");
    assert_eq!(slots[2], "#0000FF");
    assert_eq!(slots[3], "#00FF00");
}

#[test]
fn test_each_slot_has_its_name_and_the_area_of_every_copy() {
    let slots = colour_slots(&scene());
    let near = |a: f64, b: f64| (a - b).abs() < 1e-12;
    assert_eq!(slots[0].mat.as_deref(), Some("Red"));
    assert!(!slots[0].unpainted);
    assert!(near(slots[0].area, 2e-4), "{}", slots[0].area);
    assert!(slots[1].unpainted);
    assert_eq!(slots[1].mat, None);
    assert!(near(slots[1].area, 4e-4), "{}", slots[1].area);
    // Two copies: 6 cm² as placed, 10 cm² stretched (two end faces as
    // they were, four sides twice as long).
    assert!(near(slots[2].area, 16e-4), "{}", slots[2].area);
    // A mirrored copy keeps its area.
    assert!(near(slots[3].area, 6e-4), "{}", slots[3].area);
}

#[test]
fn test_an_empty_model_has_no_slot() {
    assert!(colour_slots(&Scene::new()).is_empty());
}
