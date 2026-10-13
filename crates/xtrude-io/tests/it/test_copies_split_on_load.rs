// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Component copies sharing a mesh share their contents once a document is
//! open: copies whose contents differ in the file open apart.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::document;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

fn unit_quad(s: &mut Scene) -> MeshId {
    let mut m = Mesh::new();
    quad(&mut m, 0., 1., 0., 1., 0.);
    add_mesh(&mut s.doc.meshes, m)
}

fn placed(mesh: MeshId, name: &str, x: f64, component: bool) -> Group {
    let mut g = Group::new(mesh, Some(name));
    g.xform = Some(DMat4::from_translation(v3(x, 0., 0.)));
    g.component = component;
    g
}

/// Two placements of one container mesh, each holding the children
/// `kids(child_mesh, i)` gives; `child_mesh` is one mesh for both.
fn two_copies(component: bool, kids: impl Fn(MeshId, usize) -> Vec<Group>) -> Scene {
    let mut s = Scene::new();
    let body = unit_quad(&mut s);
    let child = unit_quad(&mut s);
    for i in 0..2 {
        let mut g = placed(body, "Leg", 10. * i as f64, component);
        g.children = kids(child, i);
        s.doc.groups.push(g);
    }
    s
}

fn reopened(s: &Scene) -> Scene {
    let tmp = TmpDir::new();
    let path = tmp.join("legs.xtr");
    document::save_scene(s, &path).unwrap();
    assert_eq!(
        payload(&path)["protos"].as_array().map(Vec::len),
        Some(2),
        "the file holds the two prototypes once each",
    );
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    back
}

#[test]
fn test_copies_whose_children_differ_open_as_two_components() {
    let s = two_copies(true, |servo, i| {
        let mut kids = vec![placed(servo, "Servo", 0., true)];
        if i == 1 {
            kids.push(placed(servo, "Servo", 2., true));
        }
        kids
    });
    let back = reopened(&s);
    let (a, b) = (&back.doc.groups[0], &back.doc.groups[1]);
    assert_ne!(a.mesh, b.mesh, "the second copy gets a mesh of its own");
    assert_eq!(
        back.doc.mesh(a.mesh).faces.len(),
        back.doc.mesh(b.mesh).faces.len(),
        "with the same faces",
    );
    assert_eq!((a.children.len(), b.children.len()), (1, 2));
    assert_eq!(
        a.children[0].mesh, b.children[1].mesh,
        "the servos inside still share theirs",
    );
}

#[test]
fn test_copies_whose_children_agree_keep_one_mesh() {
    let s = two_copies(true, |servo, _| vec![placed(servo, "Servo", 0., true)]);
    let back = reopened(&s);
    assert_eq!(back.doc.groups[0].mesh, back.doc.groups[1].mesh);
}

#[test]
fn test_a_child_differing_only_in_paint_or_name_splits_the_copies() {
    for change in 0..4 {
        let s = two_copies(true, |servo, i| {
            let mut kid = placed(servo, "Servo", 0., true);
            if i == 1 {
                match change {
                    0 => kid.name = "Servo 2".into(),
                    1 => kid.hidden = true,
                    2 => kid.layer = Some("Parts".into()),
                    _ => kid.xform = Some(DMat4::from_translation(v3(0., 0.5, 0.))),
                }
            }
            vec![kid]
        });
        let back = reopened(&s);
        assert_ne!(
            back.doc.groups[0].mesh, back.doc.groups[1].mesh,
            "change {change}",
        );
    }
}

#[test]
fn test_a_difference_deep_inside_splits_every_level_above_it() {
    // Body ▸ Leg ▸ Servo ▸ Horn: one leg's servo lost its horn.
    let mut s = Scene::new();
    let (leg, servo, horn) = (unit_quad(&mut s), unit_quad(&mut s), unit_quad(&mut s));
    for i in 0..2 {
        let mut sv = placed(servo, "Servo", 0., true);
        if i == 0 {
            sv.children = vec![placed(horn, "Horn", 0., true)];
        }
        let mut lg = placed(leg, "Leg", 10. * i as f64, true);
        lg.children = vec![sv];
        s.doc.groups.push(lg);
    }
    let tmp = TmpDir::new();
    let path = tmp.join("body.xtr");
    document::save_scene(&s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    let (a, b) = (&back.doc.groups[0], &back.doc.groups[1]);
    assert_ne!(a.mesh, b.mesh, "the legs split");
    assert_ne!(a.children[0].mesh, b.children[0].mesh, "the servos split");
}

#[test]
fn test_groups_sharing_a_mesh_are_left_as_they_are() {
    let s = two_copies(false, |part, i| {
        let mut kids = vec![placed(part, "Part", 0., false)];
        if i == 1 {
            kids.clear();
            kids.push(placed(part, "Other", 3., false));
        }
        kids
    });
    let back = reopened(&s);
    let (a, b) = (&back.doc.groups[0], &back.doc.groups[1]);
    assert!(!a.is_component() && !b.is_component());
    assert_eq!(a.mesh, b.mesh, "groups never share contents: no split");
}

#[test]
fn test_a_group_sharing_a_component_mesh_does_not_split_it() {
    let mut s = Scene::new();
    let body = unit_quad(&mut s);
    let part = unit_quad(&mut s);
    let mut comp = placed(body, "Leg", 0., true);
    comp.children = vec![placed(part, "Part", 0., false)];
    let mut copy = placed(body, "Leg", 10., true);
    copy.children = vec![placed(part, "Part", 0., false)];
    let group = placed(body, "Group", 20., false);
    s.doc.groups.extend([comp, copy, group]);
    let tmp = TmpDir::new();
    let path = tmp.join("mixed.xtr");
    document::save_scene(&s, &path).unwrap();
    let mut back = Scene::new();
    document::load_into(&mut back, &path, None).unwrap();
    let meshes: Vec<MeshId> = back.doc.groups.iter().map(|g| g.mesh).collect();
    assert_eq!(meshes[0], meshes[1]);
    assert_eq!(meshes[0], meshes[2], "the group is not a copy: left alone");
}
