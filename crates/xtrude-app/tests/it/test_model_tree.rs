// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Panel tests: the tree mirrors the nesting sorted by name, a click opens the
//! parents and selects, the viewport's selection shows, double-click edits,
//! rename and hide are undoable, the filter keeps the groups above a match,
//! and a drop moves rows without moving them in the world. (The core half is
//! in xtrude-model.)

use crate::common;

use common::{v3, window};
use std::collections::BTreeMap;
use xtrude_app::tray::model_tree::ModelTree;
use xtrude_app::window::MainWindow;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_model::commands::MakeNestedGroupCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, iter_placements, Group};
use xtrude_model::ids::GroupId;

fn add_box(w: &mut MainWindow, x: f64, name: &str) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ],
        &[],
    );
    let g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name));
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    id
}

/// Leg ▸ (Servo-10, Servo-2), and a hidden Guide at the top level.
struct Fill {
    w: MainWindow,
    ol: ModelTree,
    leg: GroupId,
    s10: GroupId,
    s2: GroupId,
    guide: GroupId,
}

fn fill() -> Fill {
    let mut w = window();
    let s10 = add_box(&mut w, 0., "Servo-10");
    let s2 = add_box(&mut w, 3., "Servo-2");
    let guide = add_box(&mut w, 6., "Guide");
    let mut c = MakeNestedGroupCommand::new(
        w.viewport.scene.mesh(),
        &[],
        &[],
        vec![s10, s2],
        Some("Leg"),
    );
    w.viewport.execute(&mut c);
    w.viewport.scene.doc.find_group_mut(guide).unwrap().hidden = true;
    w.viewport.scene.version += 1;
    let leg = w.viewport.scene.doc.groups.last().unwrap().id;
    let mut ol = ModelTree::default();
    ol.refresh(&w);
    Fill {
        w,
        ol,
        leg,
        s10,
        s2,
        guide,
    }
}

fn world_points(w: &MainWindow) -> BTreeMap<String, Vec<(i64, i64, i64)>> {
    let doc = &w.viewport.scene.doc;
    let mut out = BTreeMap::new();
    for top in &doc.groups {
        for (g, m) in iter_placements(top, None) {
            let mut pts: Vec<(i64, i64, i64)> = doc
                .mesh(g.mesh)
                .verts
                .iter()
                .map(|(_, v)| {
                    let p = m.map_or(v.position, |m| m.project_point(v.position));
                    let r = |x: f64| (x * 1e6).round() as i64;
                    (r(p.x), r(p.y), r(p.z))
                })
                .collect();
            pts.sort();
            out.insert(g.name.clone(), pts);
        }
    }
    out
}

#[test]
fn test_the_tree_mirrors_the_nesting_sorted_by_name() {
    let f = fill();
    assert_eq!(
        f.ol.lines(),
        vec!["Guide", "Leg", "  Servo-2", "  Servo-10"]
    );
    assert!(f.ol.row(f.guide).unwrap().hidden); // hidden reads greyed, italic
}

#[test]
fn test_clicking_a_nested_row_opens_its_parent_and_selects_it() {
    let mut f = fill();
    f.ol.tree_selection(&mut f.w, &[f.s2], Some(f.s2), false);
    assert_eq!(f.w.viewport.scene.edit_group(), Some(f.leg));
    let sel: Vec<_> = f.w.viewport.scene.selection.iter().copied().collect();
    assert_eq!(sel, vec![EntityRef::Group(f.s2)]);
    // The root row: back to the model, nothing selected.
    f.ol.tree_selection(&mut f.w, &[], None, true);
    assert!(f.w.viewport.scene.edit_group().is_none() && f.w.viewport.scene.selection.is_empty());
}

#[test]
fn test_the_viewport_selection_shows_in_the_tree() {
    let mut f = fill();
    f.w.viewport.scene.selection.insert(EntityRef::Group(f.leg));
    f.w.viewport.scene.bump_view();
    f.ol.refresh(&f.w);
    assert_eq!(f.ol.selected, vec![f.leg]);
}

#[test]
fn test_double_click_edits_the_group() {
    let mut f = fill();
    f.ol.edit_group(&mut f.w, f.s10);
    let stack: Vec<GroupId> =
        f.w.viewport
            .scene
            .edit_stack
            .iter()
            .map(|l| l.group)
            .collect();
    assert_eq!(stack, vec![f.leg, f.s10]);
    assert!(f.ol.open.contains(&f.s10)); // shown bold
}

#[test]
fn test_renaming_a_row_renames_the_group_undoably() {
    let mut f = fill();
    f.ol.rename(&mut f.w, f.leg, "Front leg");
    assert_eq!(
        f.w.viewport.scene.doc.find_group(f.leg).unwrap().name,
        "Front leg"
    );
    f.w.viewport.undo();
    f.ol.refresh(&f.w);
    assert_eq!(
        f.w.viewport.scene.doc.find_group(f.leg).unwrap().name,
        "Leg"
    );
    assert_eq!(f.ol.row(f.leg).unwrap().name, "Leg");
}

#[test]
fn test_hide_and_unhide_from_the_model_tree() {
    let mut f = fill();
    f.ol.set_hidden(&mut f.w, &[f.guide], false);
    assert!(!f.w.viewport.scene.doc.find_group(f.guide).unwrap().hidden);
    assert!(!f.ol.row(f.guide).unwrap().hidden);
    f.ol.set_hidden(&mut f.w, &[f.leg], true);
    assert!(f.w.viewport.scene.doc.find_group(f.leg).unwrap().hidden);
}

#[test]
fn test_the_filter_keeps_matches_and_the_groups_above_them() {
    let mut f = fill();
    f.ol.filter = "servo-1".into();
    assert!(f.ol.shown(f.s10));
    assert!(f.ol.shown(f.leg));
    assert!(!f.ol.shown(f.s2));
    assert!(!f.ol.shown(f.guide));
    f.ol.filter.clear();
    assert!([f.leg, f.s10, f.s2, f.guide].iter().all(|g| f.ol.shown(*g)));
}

#[test]
fn test_dropping_a_row_on_a_group_moves_it_there() {
    let mut f = fill();
    let before = world_points(&f.w);
    f.ol.selected = vec![f.guide];
    assert!(f.ol.can_drop(&f.w, Some(f.leg)));
    assert!(f.ol.drop(&mut f.w, Some(f.leg), None));
    let leg = f.w.viewport.scene.doc.find_group(f.leg).unwrap();
    assert!(leg.children.iter().any(|c| c.id == f.guide));
    assert!(!f
        .w
        .viewport
        .scene
        .doc
        .groups
        .iter()
        .any(|g| g.id == f.guide));
    assert_eq!(world_points(&f.w), before);
    assert_eq!(
        f.ol.lines(),
        vec!["Leg", "  Guide", "  Servo-2", "  Servo-10"]
    );
    f.w.viewport.undo();
    f.ol.refresh(&f.w);
    assert!(f
        .w
        .viewport
        .scene
        .doc
        .groups
        .iter()
        .any(|g| g.id == f.guide));
    assert_eq!(f.ol.lines()[0], "Guide");
}

#[test]
fn test_a_row_cannot_be_dropped_into_its_own_subtree() {
    let mut f = fill();
    f.ol.selected = vec![f.leg];
    assert!(!f.ol.can_drop(&f.w, Some(f.s10)));
    assert!(!f.ol.drop(&mut f.w, Some(f.s10), None));
    assert!(f.w.viewport.scene.doc.groups.iter().any(|g| g.id == f.leg));
}
