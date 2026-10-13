// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Components that place other components — one prototype mesh, a matrix per
//! placement — through queries, copies, the edit stack, Explode and the
//! viewport's placement expansion. The document test lives in crates/xtrude-io.

use crate::common;
use crate::common::FakeProbe;

use common::stub;
use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_io::meshexport::world_faces;
use xtrude_model::commands::ExplodeGroupCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, iter_placements, materialize, world_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::layers::Layer;

fn unit_quad(m: &mut Mesh, z: f64) {
    m.add_face(
        &[
            DVec3::new(0., 0., z),
            DVec3::new(1., 0., z),
            DVec3::new(1., 1., z),
            DVec3::new(0., 1., z),
        ],
        &[],
    );
}

fn at(x: f64, y: f64, z: f64) -> DMat4 {
    DMat4::from_translation(DVec3::new(x, y, z))
}

/// One prototype quad, placed twice inside a child that is itself placed
/// inside the top-level group — two levels of nesting over ONE mesh.
fn tree(vp: &mut Viewport) -> (Group, MeshId) {
    let mut m = Mesh::new();
    unit_quad(&mut m, 0.0);
    let proto = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut a = Group::new(proto, Some("leaf A"));
    a.xform = Some(at(0., 0., 0.));
    let mut b = Group::new(proto, Some("leaf B"));
    b.xform = Some(at(10., 0., 0.));
    let mut mid = Group::new(proto, Some("mid"));
    mid.xform = Some(at(0., 100., 0.));
    mid.children = vec![a, b];
    let mut top = Group::new(proto, Some("top"));
    top.xform = Some(at(0., 0., 1000.));
    top.children = vec![mid];
    (top, proto)
}

fn translation(m: Option<DMat4>) -> (f64, f64, f64) {
    let t = m.unwrap().w_axis;
    (t.x, t.y, t.z)
}

#[test]
fn test_iter_placements_composes_the_chain() {
    let mut vp = stub();
    let (top, proto) = tree(&mut vp);
    let seen: Vec<(String, (f64, f64, f64))> = iter_placements(&top, None)
        .into_iter()
        .map(|(g, m)| (g.name.clone(), translation(m)))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("top".into(), (0., 0., 1000.)),
            ("mid".into(), (0., 100., 1000.)),
            ("leaf A".into(), (0., 100., 1000.)),
            ("leaf B".into(), (10., 100., 1000.)),
        ]
    );
    assert!(iter_placements(&top, None)
        .iter()
        .all(|(g, _)| g.mesh == proto)); // ONE mesh
}

#[test]
fn test_world_mesh_folds_children_without_touching_the_prototype() {
    let mut vp = stub();
    let (top, proto) = tree(&mut vp);
    let world = world_mesh(&vp.scene.doc.meshes, &top);
    assert_eq!(vp.scene.doc.mesh(proto).faces.len(), 1); // stored once…
    assert_eq!(world.faces.len(), 4); // …drawn four times
    let mut xs: Vec<i64> = world
        .verts
        .iter()
        .map(|(_, v)| v.position.x.round() as i64)
        .collect();
    xs.sort();
    xs.dedup();
    let mut zs: Vec<i64> = world
        .verts
        .iter()
        .map(|(_, v)| v.position.z.round() as i64)
        .collect();
    zs.sort();
    zs.dedup();
    assert!(xs == vec![0, 1, 10, 11] && zs == vec![1000]);
}

#[test]
fn test_copy_group_keeps_the_nesting_and_the_shared_prototype() {
    let mut vp = stub();
    let (mut top, proto) = tree(&mut vp);
    let dup = copy_group(
        &mut vp.scene.doc.meshes,
        &mut top,
        Some(DVec3::new(0., 0., 7.)),
        false,
    );
    assert!(dup.children.len() == 1 && dup.children[0].children.len() == 2);
    assert!(iter_placements(&dup, None)
        .iter()
        .all(|(g, _)| g.mesh == proto));
    let zs: std::collections::BTreeSet<i64> = world_mesh(&vp.scene.doc.meshes, &dup)
        .verts
        .iter()
        .map(|(_, v)| v.position.z.round() as i64)
        .collect();
    assert_eq!(zs, [1007].into_iter().collect());
    assert_eq!(world_mesh(&vp.scene.doc.meshes, &top).faces.len(), 4); // source untouched
}

#[test]
fn test_materialize_bakes_children_in_and_drops_them() {
    let mut vp = stub();
    let (mut top, _) = tree(&mut vp);
    materialize(&mut vp.scene.doc.meshes, &mut top);
    assert!(top.xform.is_none() && top.children.is_empty());
    assert_eq!(vp.scene.doc.mesh(top.mesh).faces.len(), 4); // nothing drawn twice
}

fn with_tree(vp: &mut Viewport) -> (GroupId, MeshId) {
    let (top, proto) = tree(vp);
    let id = top.id;
    vp.scene.doc.groups.push(top);
    vp.scene.version += 1;
    (id, proto)
}

#[test]
fn test_placements_expand_for_draw_and_map_back_to_the_owner() {
    let mut vp = stub();
    let (top, _) = with_tree(&mut vp);
    let out = vp.placements();
    assert_eq!(out.len(), 4); // every placement draws
    assert_eq!(out[0].group, top);
    assert!(out.iter().all(|p| p.owner == top)); // ONE object to click
    let ts: Vec<(f64, f64, f64)> = out[1..].iter().map(|p| translation(p.xform)).collect();
    assert_eq!(
        ts,
        vec![(0., 100., 1000.), (0., 100., 1000.), (10., 100., 1000.)]
    );
}

#[test]
fn test_placement_proxies_are_stable_across_frames() {
    // Rust placements are values keyed by group id: the same ids, in the
    // same order, frame after frame — and fresh matrices after a move.
    let mut vp = stub();
    let (top, _) = with_tree(&mut vp);
    let first: Vec<GroupId> = vp.placements().iter().map(|p| p.group).collect();
    assert_eq!(
        vp.placements().iter().map(|p| p.group).collect::<Vec<_>>(),
        first
    );
    vp.scene.doc.find_group_mut(top).unwrap().xform = Some(at(0., 0., 2000.)); // a Move drag
    let again = vp.placements();
    assert_eq!(again.iter().map(|p| p.group).collect::<Vec<_>>(), first);
    assert_eq!(translation(again[1].xform).2, 2000.0);
}

#[test]
fn test_flat_scenes_take_the_untouched_fast_path() {
    let mut vp = stub();
    let mut m = Mesh::new();
    unit_quad(&mut m, 0.0);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let plain = Group::new(mid, Some("plain"));
    let id = plain.id;
    vp.scene.doc.groups.push(plain);
    let out = vp.placements();
    assert!(out.len() == 1 && out[0].group == id && !out[0].nested);
}

#[test]
fn test_hidden_owner_hides_its_nested_placements() {
    let mut vp = stub();
    let (mut top, _) = tree(&mut vp);
    top.layer = Some("hidden".into());
    top.children[0].layer = Some("visible".into()); // a differently tagged child
    let mut oc = Layer::new("hidden");
    oc.visible = false;
    vp.scene.doc.layers.push(oc);
    vp.scene.doc.layers.push(Layer::new("visible"));
    vp.scene.doc.groups.push(top);
    let out = vp.placements();
    let visible = |p: &xtrude_app::viewport::Placement| {
        !p.hidden
            && p.layer
                .as_deref()
                .and_then(|n| vp.scene.doc.layers.iter().find(|l| l.name == n))
                .is_none_or(|l| l.visible)
    };
    assert!(!out.iter().any(visible));
}

fn world_points(vp: &Viewport, g: &Group) -> Vec<(i64, i64, i64)> {
    iter_placements(g, None)
        .iter()
        .flat_map(|(pg, m)| {
            let mesh = vp.scene.doc.mesh(pg.mesh);
            mesh.verts
                .iter()
                .map(|(_, vx)| {
                    let p = m.map_or(vx.position, |m| m.project_point(vx.position));
                    (
                        (p.x * 1e6).round() as i64,
                        (p.y * 1e6).round() as i64,
                        (p.z * 1e6).round() as i64,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn test_editing_into_a_nested_group_keeps_the_tree() {
    let mut vp = stub();
    let (top, proto) = with_tree(&mut vp);
    let before = world_points(&vp, vp.scene.doc.find_group(top).unwrap());
    vp.scene.begin_group_edit(top);
    let g = vp.scene.doc.find_group(top).unwrap();
    assert_eq!(vp.scene.active_mesh_id(), g.mesh);
    assert_eq!(g.children.len(), 1, "the tree was baked on entry");
    assert_eq!(g.children[0].name, "mid");
    assert_eq!(
        g.children[0].children[0].mesh, proto,
        "and still shares its mesh"
    );
    assert_eq!(g.xform, Some(DMat4::IDENTITY));
    let after = world_points(&vp, g);
    assert_eq!(before, after, "the geometry moved on entry");
}

#[test]
fn test_scene_queries_see_the_nested_geometry() {
    let mut vp = stub();
    with_tree(&mut vp);
    assert_eq!(vp.scene.placements().len(), 4);
    assert_eq!(vp.scene.render_faces().len(), 4);
    assert_eq!(world_faces(&vp.scene, true).len(), 4);
    let (lo, hi) = vp.scene.bounds().unwrap();
    assert_eq!((lo.x.round(), hi.x.round()), (0., 11.)); // leaf B at x=10
    assert_eq!((lo.y.round(), hi.y.round()), (0., 101.)); // mid at y=100
    assert_eq!((lo.z.round(), hi.z.round()), (1000., 1000.));
}

#[test]
fn test_a_group_that_owns_placements_is_always_an_instance() {
    let mut vp = stub();
    let mut m = Mesh::new();
    unit_quad(&mut m, 0.0);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut plain = Group::new(mid, Some("classic"));
    let mut kid = Group::new(mid, Some("kid"));
    kid.xform = Some(at(3., 0., 0.));
    assert!(plain.xform.is_none());
    plain.adopt(vec![kid]);
    assert!(plain.xform.is_some());
    assert_eq!(world_mesh(&vp.scene.doc.meshes, &plain).faces.len(), 2);
}

#[test]
fn test_the_drag_preview_carries_the_whole_subtree() {
    let mut vp = stub();
    let (top, _) = with_tree(&mut vp);
    let g = vp.scene.doc.find_group(top).unwrap().clone();
    let mut movers = Vec::new();
    vp.expand_placements(&g, &mut movers);
    assert_eq!(movers.len(), 4); // every placement moves
    assert_eq!(movers[0].group, top);
    let faces: usize = movers
        .iter()
        .map(|p| vp.scene.doc.mesh(p.mesh).faces.len())
        .sum();
    assert_eq!(faces, 4);
    // and the same entries the draw list uses
    assert_eq!(
        movers.iter().map(|p| p.group).collect::<Vec<_>>(),
        vp.placements().iter().map(|p| p.group).collect::<Vec<_>>()
    );
}

/// A window around the nested geometry takes the whole component.
#[test]
fn test_box_selection_reaches_nested_geometry() {
    use xtrude_app::tools::select::SelectTool;
    use xtrude_app::tools::Tool;
    use xtrude_model::scene::SelectMode;
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x, p.y)));
    let (top, _) = with_tree(&mut vp);
    SelectTool::new().on_box_select(&mut vp, (-1., -1., 12., 102.), false, SelectMode::Replace);
    let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
    assert_eq!(sel, vec![EntityRef::Group(top)]);
}

#[test]
fn test_explode_removes_one_level() {
    let mut vp = stub();
    let (top, proto) = with_tree(&mut vp);
    let mid = vp.scene.doc.find_group(top).unwrap().children[0].id;
    vp.run(ExplodeGroupCommand { group: top });
    let ids: Vec<GroupId> = vp.scene.doc.groups.iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![mid]);
    assert_eq!(vp.scene.mesh().faces.len(), 1); // only the top's own quad
    assert!(vp
        .scene
        .mesh()
        .verts
        .iter()
        .all(|(_, v)| (v.position.z - 1000.).abs() < 1e-9));
    let m = vp.scene.doc.find_group(mid).unwrap();
    assert_eq!(translation(m.xform), (0., 100., 1000.));
    assert!(m.mesh == proto && m.children.len() == 2);
    assert!(iter_placements(m, None)
        .iter()
        .all(|(g, _)| g.mesh == proto));
    let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
    assert_eq!(sel, vec![EntityRef::Group(mid)]); // what came out is selected
    assert!(vp.undo());
    let ids: Vec<GroupId> = vp.scene.doc.groups.iter().map(|g| g.id).collect();
    assert!(ids == vec![top] && vp.scene.mesh().faces.is_empty());
    let t = vp.scene.doc.find_group(top).unwrap();
    assert_eq!(
        t.children.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![mid]
    );
    assert_eq!(translation(t.children[0].xform), (0., 100., 0.));
    assert!(vp.redo());
    assert_eq!(
        translation(vp.scene.doc.find_group(mid).unwrap().xform),
        (0., 100., 1000.)
    );
}

#[test]
fn test_the_selection_box_follows_a_moved_group() {
    let mut vp = stub();
    let (top, _) = with_tree(&mut vp);
    let centre = |vp: &Viewport| {
        let (f, lo, hi) = vp.group_obb(top).unwrap();
        let cs = xtrude_model::group::oriented_box_corners(f, lo, hi);
        cs.iter().fold(DVec3::ZERO, |a, c| a + *c) / cs.len() as f64
    };
    let c0 = centre(&vp);
    let g = vp.scene.doc.find_group_mut(top).unwrap();
    g.xform = Some(at(7., 0., 0.) * g.xform.unwrap()); // a Move drag
    vp.scene.version += 1;
    let c1 = centre(&vp);
    assert!((c1 - c0 - DVec3::new(7., 0., 0.)).length() < 1e-6);
}
