// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The links of a robot description: one per top-level group, named after
//! it with only `[A-Za-z0-9_]`, made unique, `base_link` kept for loose
//! geometry; and where a nested group's points are in the model.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::links::{links, sanitize, BASE_LINK};
use xtrude_model::scene::Scene;

fn square(at: DVec3) -> Mesh {
    let mut m = Mesh::new();
    m.add_face(
        &[
            at,
            at + V3(1., 0., 0.),
            at + V3(1., 1., 0.),
            at + V3(0., 1., 0.),
        ],
        &[],
    );
    m
}

fn group(scene: &mut Scene, name: &str) -> Group {
    let mesh = add_mesh(&mut scene.doc.meshes, square(DVec3::ZERO));
    Group::new(mesh, Some(name))
}

fn names(scene: &Scene) -> Vec<String> {
    links(&scene.doc).into_iter().map(|l| l.name).collect()
}

#[test]
fn test_sanitize_keeps_letters_digits_and_underscores() {
    assert_eq!(sanitize("arm_2"), "arm_2");
    assert_eq!(sanitize("upper arm"), "upper_arm");
    assert_eq!(sanitize("  gripper-left.1 "), "gripper_left_1");
    assert_eq!(sanitize("Gelenk ä"), "Gelenk__");
    assert_eq!(sanitize(""), "link");
    assert_eq!(sanitize("   "), "link");
}

#[test]
fn test_one_link_per_top_level_group_in_order() {
    let (mut scene, _) = scene_history();
    let mut base = group(&mut scene, "base");
    let finger = group(&mut scene, "finger");
    base.adopt(vec![finger]);
    let arm = group(&mut scene, "arm");
    let ids = [base.id, arm.id];
    scene.doc.groups.push(base);
    scene.doc.groups.push(arm);
    let got = links(&scene.doc);
    assert_eq!(got.iter().map(|l| l.group).collect::<Vec<_>>(), ids);
    assert_eq!(names(&scene), ["base", "arm"]); // nested groups merge
}

#[test]
fn test_names_that_collide_take_a_suffix() {
    let (mut scene, _) = scene_history();
    for name in ["arm 1", "arm_1", "arm-1", "arm_1_2"] {
        let g = group(&mut scene, name);
        scene.doc.groups.push(g);
    }
    assert_eq!(names(&scene), ["arm_1", "arm_1_2", "arm_1_3", "arm_1_2_2"]);
}

#[test]
fn test_base_link_is_kept_only_when_loose_geometry_has_faces() {
    let (mut scene, _) = scene_history();
    let g = group(&mut scene, BASE_LINK);
    scene.doc.groups.push(g);
    assert_eq!(names(&scene), [BASE_LINK]);
    let loose = scene.doc.meshes.get_mut(&MeshId::LOOSE).unwrap();
    std::sync::Arc::make_mut(loose)
        .add_face(&[V3(5., 0., 0.), V3(6., 0., 0.), V3(6., 1., 0.)], &[]);
    assert_eq!(names(&scene), ["base_link_2"]);
}

#[test]
fn test_a_nested_groups_points_take_its_parents_placements() {
    let (mut scene, _) = scene_history();
    let mut inner = group(&mut scene, "inner");
    inner.xform = Some(DMat4::from_translation(V3(0., 10., 0.)));
    let inner_id = inner.id;
    let mut outer = group(&mut scene, "outer");
    outer.xform = Some(DMat4::from_translation(V3(100., 0., 0.)));
    outer.adopt(vec![inner]);
    let outer_id = outer.id;
    scene.doc.groups.push(outer);
    let mut pts = scene.doc.group_world_points(inner_id);
    pts.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
    assert_eq!(pts[0], V3(100., 10., 0.));
    assert_eq!(pts[3], V3(101., 11., 0.));
    // the outer group's own points and its child's
    assert_eq!(scene.doc.group_world_points(outer_id).len(), 8);
    let stray = group(&mut scene, "not in the model").id;
    assert!(scene.doc.group_world_points(stray).is_empty());
    let path: Vec<_> = scene
        .doc
        .group_path(inner_id)
        .unwrap()
        .iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(path, [outer_id, inner_id]);
}
