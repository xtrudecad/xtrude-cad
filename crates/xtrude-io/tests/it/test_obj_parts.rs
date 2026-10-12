// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An OBJ made of named parts imports as a container of parts.

use crate::common;
use common::*;
use std::collections::BTreeSet;
use xtrude_core::math::DMat4;
use xtrude_io::obj::{load_obj, tidy_blender_name};
use xtrude_model::commands::ExplodeGroupCommand;
use xtrude_model::group::{world_mesh, Group};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

fn cube(x0: f64, z0: f64, base: usize) -> String {
    let mut pts = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                pts.push((x0 + dx, dy, z0 + dz));
            }
        }
    }
    let quads = [
        [1, 3, 4, 2],
        [5, 6, 8, 7],
        [1, 2, 6, 5],
        [3, 7, 8, 4],
        [1, 5, 7, 3],
        [2, 4, 8, 6],
    ];
    let mut lines: Vec<String> = pts
        .iter()
        .map(|(x, y, z)| format!("v {x} {y} {z}"))
        .collect();
    for q in quads {
        lines.push(format!(
            "f {}",
            q.iter()
                .map(|i| (base + i).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    lines.join("\n")
}

fn write(tmp: &TmpDir, body: &str) -> std::path::PathBuf {
    let p = tmp.join("piece.obj");
    std::fs::write(&p, format!("{body}\n")).unwrap();
    p
}

fn two_stacked(tmp: &TmpDir, names: (&str, &str)) -> std::path::PathBuf {
    write(
        tmp,
        &format!(
            "g {}\n{}\ng {}\n{}",
            names.0,
            cube(0., 1., 0),
            names.1,
            cube(0., 0., 8)
        ),
    )
}

#[test]
fn test_groups_in_the_file_become_named_children() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_obj(&mut s, &two_stacked(&tmp, ("seat", "leg")), None, 1.0, None).unwrap();
    assert!(s.mesh().faces.is_empty());
    assert_eq!(s.doc.groups.len(), 1);
    let stool = &s.doc.groups[0];
    assert_eq!(stool.name, "piece");
    assert_eq!(
        stool
            .children
            .iter()
            .map(|k| k.name.as_str())
            .collect::<Vec<_>>(),
        ["seat", "leg"]
    );
    assert!(stool.is_instance());
}

#[test]
fn test_touching_parts_are_not_welded_together() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_obj(&mut s, &two_stacked(&tmp, ("seat", "leg")), None, 1.0, None).unwrap();
    for kid in &s.doc.groups[0].children {
        assert_eq!(s.doc.mesh(kid.mesh).faces.len(), 6);
    }
}

#[test]
fn test_numbered_groups_read_as_parts() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_obj(&mut s, &two_stacked(&tmp, ("1", "2")), None, 1.0, None).unwrap();
    let names: Vec<&str> = s.doc.groups[0]
        .children
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    assert_eq!(names, ["Part 1", "Part 2"]);
}

#[test]
fn test_a_single_piece_file_imports_as_before() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_obj(
        &mut s,
        &write(&tmp, &format!("g only\n{}", cube(0., 0., 0))),
        None,
        1.0,
        None,
    )
    .unwrap();
    assert!(s.doc.groups.is_empty());
    assert_eq!(s.mesh().faces.len(), 6);
}

fn world_points(s: &Scene, groups: &[&Group]) -> BTreeSet<(i64, i64, i64)> {
    groups
        .iter()
        .flat_map(|g| {
            world_mesh(&s.doc.meshes, g)
                .verts
                .iter()
                .map(|(_, v)| v.position)
                .collect::<Vec<_>>()
        })
        .map(|p| {
            (
                (p.x * 1e6).round() as i64,
                (p.y * 1e6).round() as i64,
                (p.z * 1e6).round() as i64,
            )
        })
        .collect()
}

#[test]
fn test_explode_frees_the_parts_where_they_stood() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_obj(&mut s, &two_stacked(&tmp, ("seat", "leg")), None, 1.0, None).unwrap();
    let m = DMat4::from_translation(v3(5., 2., 0.));
    let x = s.doc.groups[0].xform.unwrap();
    s.doc.groups[0].xform = Some(m * x);
    let stool = s.doc.groups[0].clone();
    let before = world_points(&s, &[&stool]);
    let mut h = History::new();
    h.execute(&mut s, &mut ExplodeGroupCommand { group: stool.id });
    assert!(s.doc.find_group(stool.id).is_none());
    assert_eq!(
        s.doc
            .groups
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        ["seat", "leg"]
    );
    assert!(s.mesh().faces.is_empty());
    let all: Vec<&Group> = s.doc.groups.iter().collect();
    assert_eq!(world_points(&s, &all), before);
    h.undo(&mut s);
    assert_eq!(s.doc.groups.len(), 1);
    assert_eq!(s.doc.groups[0].id, stool.id);
    let kids: Vec<&str> = s.doc.groups[0]
        .children
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    assert_eq!(kids, ["seat", "leg"]);
    assert!(s.doc.groups[0]
        .children
        .iter()
        .all(|k| k.xform == Some(DMat4::IDENTITY)));
}

#[test]
fn test_blender_bookkeeping_leaves_the_part_names() {
    assert_eq!(
        tidy_blender_name("Base-unit-Box-Right_Cube.002"),
        "Base-unit-Box-Right"
    );
    assert_eq!(
        tidy_blender_name("Door-and-Drawers-Right.001_Cube.007"),
        "Door-and-Drawers-Right"
    );
    assert_eq!(tidy_blender_name("Sphere.021_Sphere.005"), "Sphere");
    assert_eq!(tidy_blender_name("left_leg"), "left_leg");
    assert_eq!(tidy_blender_name("seat"), "seat");
}
