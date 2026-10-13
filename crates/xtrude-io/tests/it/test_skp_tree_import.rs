// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A `.skp` of modest size keeps its instance tree, one Group per placement,
//! nested as in the file, with the instance's own name, hidden flag and kind.

use crate::common;

use common::skp::*;
use openskp::{Instance, SkpModel};
use xtrude_core::math::MatExt;
use xtrude_core::DVec3;
use xtrude_io::skp::{self, adapter, AdaptOptions};
use xtrude_model::group::{iter_placements, Group};
use xtrude_model::scene::Scene;

fn named(mut i: Instance, name: &str) -> Instance {
    i.name = name.into();
    i
}

/// body ▸ (leg-1, leg-2) ▸ servo (+ a hidden, unnamed guide group).
fn robot() -> SkpModel {
    let mut guide = named(ins(7, 0.0), "guide");
    guide.hidden = true;
    let servo = tri_def(6, "sts3215", vec![guide]);
    let leg = empty_defn(
        5,
        "leg-link",
        vec![named(ins(6, 0.0), "servo-a"), ins(6, 20.0)],
    );
    let body = empty_defn(
        4,
        "body",
        vec![named(ins(5, 0.0), "leg-1"), named(ins(5, 100.0), "leg-2")],
    );
    let root = empty_defn(0, "ROOT_MODEL", vec![named(ins(4, 0.0), "body")]);
    model(root, vec![body, leg, servo, tri_def(7, "Group#3", vec![])])
}

fn scene_of(m: &SkpModel) -> Scene {
    let mut scene = Scene::new();
    let payload = adapter::adapt(m, "robot", None, &AdaptOptions::default())
        .unwrap()
        .unwrap();
    skp::apply_payload(&mut scene, &payload).unwrap();
    scene
}

fn tree(g: &Group, depth: usize, out: &mut Vec<String>) {
    out.push(format!("{}{}", "  ".repeat(depth), g.name));
    for c in &g.children {
        tree(c, depth + 1, out);
    }
}

#[test]
fn test_the_instance_tree_comes_in_nested_and_named() {
    let scene = scene_of(&robot());
    assert_eq!(scene.doc.groups.len(), 1);
    let mut out = Vec::new();
    tree(&scene.doc.groups[0], 0, &mut out);
    assert_eq!(
        out,
        [
            "body",
            "  leg-1",
            "    servo-a",
            "      guide",
            "    sts3215",
            "      guide",
            "  leg-2",
            "    servo-a",
            "      guide",
            "    sts3215",
            "      guide",
        ]
    );
}

#[test]
fn test_hidden_instances_stay_hidden_and_groups_are_groups() {
    let scene = scene_of(&robot());
    let servo = &scene.doc.groups[0].children[0].children[0];
    let guide = &servo.children[0];
    assert!(guide.hidden && !servo.hidden);
    assert!(servo.is_component() && !guide.is_component());
    // An unnamed component instance shows its definition's name.
    assert_eq!(scene.doc.groups[0].children[0].children[1].name, "sts3215");
}

#[test]
fn test_copies_share_one_prototype_mesh_and_land_where_the_file_puts_them() {
    let scene = scene_of(&robot());
    let servos: Vec<(&Group, xtrude_core::math::DMat4)> = scene
        .doc
        .groups
        .iter()
        .flat_map(|top| iter_placements(top, None))
        .filter(|(g, _)| g.name == "servo-a" || g.name == "sts3215")
        .map(|(g, m)| (g, m.unwrap_or_default()))
        .collect();
    assert_eq!(servos.len(), 4);
    assert!(servos.iter().all(|(g, _)| g.mesh == servos[0].0.mesh));
    let mut xs: Vec<f64> = servos
        .iter()
        .map(|(_, m)| (m.project_point(DVec3::ZERO).x * 1e4).round() / 1e4)
        .collect();
    xs.sort_by(f64::total_cmp);
    // leg-1 at 0 and leg-2 at +100 in, each placing a servo at 0 and +20 in.
    assert_eq!(xs, [0.0, 0.508, 2.54, 3.048]);
}

#[test]
fn test_copies_holding_an_unnamed_group_stay_one_component() {
    // leg ▸ (an unnamed group, placed twice), the leg placed three times.
    let leg = empty_defn(5, "leg-link", vec![ins(7, 0.0), ins(7, 20.0)]);
    let root = empty_defn(
        0,
        "ROOT_MODEL",
        vec![
            named(ins(5, 0.0), "leg-1"),
            named(ins(5, 100.0), "leg-2"),
            named(ins(5, 200.0), "leg-3"),
        ],
    );
    let scene = scene_of(&model(root, vec![leg, tri_def(7, "Group#3", vec![])]));
    let legs = &scene.doc.groups;
    assert_eq!(legs.len(), 3);
    assert!(legs
        .iter()
        .all(|g| g.is_component() && g.mesh == legs[0].mesh));
    let names = |g: &Group| {
        g.children
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
    };
    let first = names(&legs[0]);
    // Each unnamed group gets a "Group N" of its own, the same in every leg.
    assert!(first.iter().all(|n| n.starts_with("Group ")));
    assert_ne!(first[0], first[1]);
    assert!(legs.iter().all(|g| names(g) == first));
}

#[test]
fn test_an_instance_tag_rides_on_its_placement() {
    let child = tri_def(5, "part", vec![]);
    let mut a = named(ins(5, 0.0), "a");
    a.layer = "Hardware".into();
    let root = empty_defn(0, "ROOT_MODEL", vec![a, named(ins(5, 0.0), "b")]);
    let scene = scene_of(&model(root, vec![child]));
    let by_name = |n: &str| scene.doc.groups.iter().find(|g| g.name == n).unwrap();
    assert_eq!(by_name("a").layer.as_deref(), Some("Hardware"));
    assert_eq!(by_name("b").layer, None);
    assert_eq!(by_name("a").mesh, by_name("b").mesh);
}

#[test]
fn test_a_large_model_still_takes_the_flattened_path() {
    let options = AdaptOptions {
        tree_max_placements: 3,
        ..Default::default()
    };
    let payload = adapter::adapt(&robot(), "robot", None, &options)
        .unwrap()
        .unwrap();
    assert!(payload.placements.is_empty());
}
