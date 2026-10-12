// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Entering a group that holds groups without fusing them
//! — the edit stack, what is touchable and what fades at each level, snapping
//! to the rest of the model, deleting and box-selecting children, and what the
//! frame fades.

use crate::common;
use crate::common::FakeProbe;
use xtrude_app::input::ScreenPt;
use xtrude_app::viewport::RestMode;

use common::{frame_triangles, sized_viewport, stub, v3};
use glam::{DMat4, DVec3};
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::render::EDIT_REST_FADE;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::commands::DeleteGroupCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, oriented_box_corners, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::{Scene, SelectMode};
use xtrude_render::frame::{Source, Stipple};

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn square(x0: f64, side: f64) -> Mesh {
    let mut m = Mesh::new();
    m.add_face(
        &[v(x0, 0.), v(x0 + side, 0.), v(x0 + side, side), v(x0, side)],
        &[],
    );
    m
}

fn group(s: &mut Scene, m: Mesh, name: &str) -> Group {
    let mid = add_mesh(&mut s.doc.meshes, m);
    Group::new(mid, Some(name))
}

/// A container with two children: an assembly with a housing and a plate.
fn assembly(s: &mut Scene) -> (GroupId, GroupId, GroupId) {
    let h1 = group(s, square(0.0, 1.0), "Housing");
    let h2 = group(s, square(3.0, 1.0), "Plate");
    let (i1, i2) = (h1.id, h2.id);
    let mut parent = group(s, Mesh::new(), "Assembly");
    parent.adopt(vec![h1, h2]);
    let ip = parent.id;
    s.doc.groups.push(parent);
    (ip, i1, i2)
}

fn adopt(s: &mut Scene, parent: GroupId, kids: Vec<Group>) {
    let p = s.doc.find_group_mut(parent).unwrap();
    let mut c = std::mem::take(&mut p.children);
    c.extend(kids);
    p.adopt(c);
}

fn faces_of(s: &Scene, g: GroupId) -> usize {
    s.doc.mesh(s.doc.find_group(g).unwrap().mesh).faces.len()
}

fn kids(s: &Scene, g: GroupId) -> Vec<GroupId> {
    s.doc
        .find_group(g)
        .unwrap()
        .children
        .iter()
        .map(|c| c.id)
        .collect()
}

#[test]
fn test_entering_does_not_fuse_the_children() {
    let mut s = Scene::new();
    let (p, h1, h2) = assembly(&mut s);
    s.begin_group_edit(p);
    assert_eq!(s.edit_group(), Some(p));
    assert_eq!(
        kids(&s, p),
        vec![h1, h2],
        "the children were baked on entry"
    );
    assert!(faces_of(&s, h1) == 1 && faces_of(&s, h2) == 1);
    assert_eq!(faces_of(&s, p), 0, "the container's own mesh is empty");
    s.end_group_edit();
    assert_eq!(kids(&s, p).len(), 2);
}

#[test]
fn test_entering_a_child_pushes_one_level() {
    let mut s = Scene::new();
    let (p, h1, _h2) = assembly(&mut s);
    s.begin_group_edit(p);
    s.begin_group_edit(h1); // a double-click inside
    assert_eq!(s.edit_group(), Some(h1));
    assert_eq!(s.active_mesh_id(), s.doc.find_group(h1).unwrap().mesh);
    assert_eq!(s.edit_stack.len(), 2);
    s.end_one_group_edit(); // Esc
    assert_eq!(s.edit_group(), Some(p), "Esc leaves you INSIDE the parent");
    assert_eq!(s.active_mesh_id(), s.doc.find_group(p).unwrap().mesh);
    s.end_one_group_edit();
    assert!(s.edit_group().is_none());
}

#[test]
fn test_end_group_edit_closes_every_level() {
    let mut s = Scene::new();
    let (p, h1, _h2) = assembly(&mut s);
    s.begin_group_edit(p);
    s.begin_group_edit(h1);
    s.end_group_edit();
    assert!(s.edit_group().is_none());
    assert!(s.edit_stack.is_empty());
    assert_eq!(s.active_mesh_id(), MeshId::LOOSE, "back on the loose mesh");
}

#[test]
fn test_entering_another_group_closes_the_open_one() {
    let mut s = Scene::new();
    let (p, h1, _h2) = assembly(&mut s);
    let other = group(&mut s, square(9.0, 1.0), "Loose");
    let io = other.id;
    s.doc.groups.push(other);
    s.begin_group_edit(p);
    s.begin_group_edit(h1);
    s.begin_group_edit(io); // not a child of the open one
    assert_eq!(s.edit_group(), Some(io));
    assert_eq!(s.edit_stack.len(), 1);
}

fn world_coords(s: &Scene, g: GroupId, outer: Option<DMat4>) -> Vec<i64> {
    let grp = s.doc.find_group(g).unwrap();
    let m = s.doc.mesh(grp.mesh);
    let mut out = Vec::new();
    for (_, vx) in m.verts.iter() {
        let mut p = vx.position;
        if let Some(x) = grp.xform {
            p = x.transform_point3(p);
        }
        if let Some(o) = outer {
            p = o.transform_point3(p);
        }
        out.extend([p.x, p.y, p.z].iter().map(|c| (c * 1e6).round() as i64));
    }
    out
}

#[test]
fn test_a_moved_container_opens_without_moving() {
    let mut s = Scene::new();
    let (p, h1, h2) = assembly(&mut s);
    let t = DMat4::from_translation(v3(10., 5., 0.));
    s.doc.find_group_mut(p).unwrap().xform = Some(t);
    let mut before: Vec<i64> = [h1, h2]
        .iter()
        .flat_map(|&g| world_coords(&s, g, Some(t)))
        .collect();
    before.sort();
    s.begin_group_edit(p);
    assert_eq!(
        s.doc.find_group(p).unwrap().xform,
        Some(DMat4::IDENTITY),
        "the container must sit at identity"
    );
    let mut after: Vec<i64> = [h1, h2]
        .iter()
        .flat_map(|&g| world_coords(&s, g, None))
        .collect();
    after.sort();
    assert_eq!(before, after, "the geometry moved on entry");
}

#[test]
fn test_a_plain_group_works_as_before() {
    let mut s = Scene::new();
    let g = group(&mut s, square(0.0, 1.0), "Box");
    let (gid, gm) = (g.id, g.mesh);
    s.doc.groups.push(g);
    s.begin_group_edit(gid);
    assert!(s.active_mesh_id() == gm && s.edit_group() == Some(gid));
    s.end_group_edit();
    assert!(s.active_mesh_id() == MeshId::LOOSE && s.edit_group().is_none());
}

// ---- inside the group, its children can be touched ---------------

fn names_of_owners(vp: &Viewport, ps: &[xtrude_app::viewport::Placement]) -> Vec<String> {
    let mut n: Vec<String> = ps
        .iter()
        .map(|p| vp.scene.doc.find_group(p.owner).unwrap().name.clone())
        .collect();
    n.sort();
    n.dedup();
    n
}

#[test]
fn test_inside_the_group_the_children_are_selectable() {
    let mut vp = stub();
    let (p, _h1, _h2) = assembly(&mut vp.scene);
    let outside: Vec<GroupId> = vp
        .placements()
        .iter()
        .filter(|q| q.group != p)
        .map(|q| q.owner)
        .collect();
    assert!(
        outside.iter().all(|&o| o == p),
        "at the root all point to the container"
    );
    vp.scene.begin_group_edit(p);
    let inside = vp.context_placements();
    assert_eq!(names_of_owners(&vp, &inside), vec!["Housing", "Plate"]);
    assert!(
        !inside.iter().any(|q| q.group == p),
        "the container is not selectable inside"
    );
}

#[test]
fn test_the_context_keeps_the_rest_of_the_model_out_of_reach() {
    let mut vp = stub();
    let (p, _h1, _h2) = assembly(&mut vp.scene);
    let loose = group(&mut vp.scene, square(20.0, 1.0), "Other part");
    let loose_id = loose.id;
    vp.scene.doc.groups.push(loose);
    assert!(vp.context_placements().iter().any(|q| q.group == loose_id)); // at the root, yes
    vp.scene.begin_group_edit(p);
    assert!(
        !vp.context_placements().iter().any(|q| q.group == loose_id),
        "from inside a group the outside cannot be grabbed"
    );
}

fn placement(vp: &Viewport, name: &str) -> xtrude_app::viewport::Placement {
    vp.placements()
        .into_iter()
        .find(|q| vp.scene.doc.find_group(q.group).unwrap().name == name)
        .unwrap()
}

#[test]
fn test_the_children_do_not_fade_but_the_rest_does() {
    let mut vp = stub();
    let (p, _h1, _h2) = assembly(&mut vp.scene);
    let loose = group(&mut vp.scene, square(20.0, 1.0), "Other part");
    vp.scene.doc.groups.push(loose);
    vp.scene.begin_group_edit(p);
    let inside = vp.context_placements();
    assert!(
        !inside.iter().any(|q| vp.draws_in_edit_context(q)),
        "the open group's children are the subject, not the scenery"
    );
    assert!(
        vp.draws_in_edit_context(&placement(&vp, "Other part")),
        "the outside fades"
    );
}

#[test]
fn test_two_levels_and_esc_goes_up_one_at_a_time() {
    let mut vp = stub();
    let (p, h1, _h2) = assembly(&mut vp.scene);
    let grandchild = group(&mut vp.scene, square(6.0, 1.0), "Lid");
    adopt(&mut vp.scene, h1, vec![grandchild]);
    vp.scene.begin_group_edit(p);
    vp.scene.begin_group_edit(h1);
    assert_eq!(vp.scene.edit_group(), Some(h1));
    assert_eq!(names_of_owners(&vp, &vp.context_placements()), vec!["Lid"]);
    vp.scene.end_one_group_edit();
    assert_eq!(vp.scene.edit_group(), Some(p));
    assert_eq!(
        names_of_owners(&vp, &vp.context_placements()),
        vec!["Housing", "Plate"]
    );
}

#[test]
fn test_at_the_second_level_the_outside_fades_too() {
    let mut vp = stub();
    let (p, h1, _h2) = assembly(&mut vp.scene);
    let a = group(&mut vp.scene, square(6.0, 1.0), "Lid");
    let b = group(&mut vp.scene, square(8.0, 1.0), "Mount");
    adopt(&mut vp.scene, h1, vec![a, b]);
    vp.scene.begin_group_edit(p);
    vp.scene.begin_group_edit(h1); // two levels in
    assert_eq!(vp.scene.edit_group(), Some(h1));
    let inside = vp.context_placements();
    assert_eq!(names_of_owners(&vp, &inside), vec!["Lid", "Mount"]);
    assert!(
        !inside.iter().any(|q| vp.draws_in_edit_context(q)),
        "the grandchildren are the subject at the second level"
    );
    // and the open group's sibling does fade
    assert!(vp.draws_in_edit_context(&placement(&vp, "Plate")));
}

#[test]
fn test_the_grandchild_is_selected_only_at_its_level() {
    let mut vp = stub();
    let (p, h1, _h2) = assembly(&mut vp.scene);
    let grandchild = group(&mut vp.scene, square(6.0, 1.0), "Lid");
    adopt(&mut vp.scene, h1, vec![grandchild]);
    let owner = |vp: &Viewport| {
        let q = placement(vp, "Lid");
        vp.scene.doc.find_group(q.owner).unwrap().name.clone()
    };
    assert_eq!(owner(&vp), "Assembly");
    vp.scene.begin_group_edit(p);
    assert_eq!(owner(&vp), "Housing");
    vp.scene.begin_group_edit(h1);
    assert_eq!(owner(&vp), "Lid");
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_entering_a_NESTED_group_does_not_fade_it() {
    let mut vp = stub();
    let (_p, h1, _h2) = assembly(&mut vp.scene);
    let loose = group(&mut vp.scene, square(20.0, 1.0), "Other");
    vp.scene.doc.groups.push(loose);
    vp.scene.begin_group_edit(h1); // STRAIGHT to the child
    assert_eq!(vp.scene.edit_group(), Some(h1));
    assert!(
        !vp.scene.doc.groups.iter().any(|g| g.id == h1),
        "it lives inside the assembly"
    );
    let proxy = placement(&vp, "Housing");
    assert!(
        !vp.draws_in_edit_context(&proxy),
        "the group being edited cannot fade itself"
    );
    assert!(
        vp.draws_in_edit_context(&placement(&vp, "Other")),
        "and the outside does"
    );
    assert!(vp.draws_in_edit_context(&placement(&vp, "Plate")));
    assert!(
        !vp.context_placements()
            .iter()
            .any(|q| q.group == h1 && q.owner == h1 && q.context != Some(h1)),
        "the group you are in is not selectable"
    );
}

#[test]
fn test_the_hint_says_which_level_you_are_at() {
    let mut vp = stub();
    let (p, h1, _h2) = assembly(&mut vp.scene);
    let grandchild = group(&mut vp.scene, square(6.0, 1.0), "Lid");
    let n = grandchild.id;
    adopt(&mut vp.scene, h1, vec![grandchild]);
    assert_eq!(vp.edit_path_text(), "");
    vp.scene.begin_group_edit(p);
    assert_eq!(vp.edit_path_text(), "Assembly");
    vp.scene.begin_group_edit(h1);
    assert_eq!(vp.edit_path_text(), "Assembly › Housing");
    vp.scene.begin_group_edit(n);
    assert_eq!(vp.edit_path_text(), "Assembly › Housing › Lid");
    vp.scene.end_one_group_edit();
    assert_eq!(vp.edit_path_text(), "Assembly › Housing");
}

// ---- inside a group the rest of the model is still a reference -------------

fn picking_vp() -> Viewport {
    let mut vp = sized_viewport(640., 480.);
    vp.camera.set_view("top");
    vp.camera.fit_to(v(-1., -1.), v(9., 9.), 1.3);
    vp
}

fn assembly_with_plate(vp: &mut Viewport) -> (GroupId, GroupId, GroupId, GroupId) {
    let (p, h1, h2) = assembly(&mut vp.scene);
    let plate = group(&mut vp.scene, square(6.0, 2.0), "Plate outside");
    let ip = plate.id;
    vp.scene.doc.groups.push(plate); // (6..8, 0..2)
    (p, h1, h2, ip)
}

#[test]
fn test_inside_a_group_the_rest_of_the_model_snaps() {
    let mut vp = picking_vp();
    let (p, h1, _h2, _pav) = assembly_with_plate(&mut vp);
    vp.scene.begin_group_edit(p);
    vp.scene.selection.insert(EntityRef::Group(h1));
    let corner = v(8.0, 2.0); // the plate's, outside the assembly
    let ScreenPt { x: px, y: py } = vp.world_to_pixel(corner).unwrap();
    let got = vp.pick_vertex(px, py);
    assert!(
        got.is_some_and(|g| (g - corner).length() < 1e-4),
        "the plate's corner must be a 'from point' from inside"
    );
    let near = vp.nearby_group_edges(px, py, 48.0, 48);
    assert!(
        near.iter()
            .any(|e| (e.a - corner).length() < 1e-6 || (e.b - corner).length() < 1e-6),
        "the rest's edges must reach the snap engine"
    );
}

#[test]
fn test_the_rest_of_the_model_cannot_be_selected_from_inside() {
    let mut vp = picking_vp();
    let (p, h1, _h2, _pav) = assembly_with_plate(&mut vp);
    vp.scene.begin_group_edit(p);
    let ScreenPt { x: px, y: py } = vp.world_to_pixel(v(7.0, 1.0)).unwrap();
    assert!(vp.pick_group(px, py).is_none());
    assert!(vp.pick_face_any(px, py).is_none());
    // and its child yes, by the face and by the fallback edge
    let ScreenPt { x: px, y: py } = vp.world_to_pixel(v(0.5, 0.5)).unwrap();
    assert_eq!(vp.pick_group(px, py), Some(h1));
    let sel = vp.pick_index().group_edge_selectable().to_vec();
    assert!(!sel.is_empty());
    assert!(
        !sel.iter().all(|s| *s),
        "the rest's edges are marked not selectable"
    );
    assert!(sel.iter().any(|s| *s), "the context's are");
}

#[test]
fn test_with_the_rest_hidden_there_is_nothing_to_snap_to() {
    let mut vp = picking_vp();
    let (p, _h1, _h2, _pav) = assembly_with_plate(&mut vp);
    vp.set_edit_rest_mode(RestMode::Hide);
    vp.scene.begin_group_edit(p);
    let corner = v(8.0, 2.0);
    let ScreenPt { x: px, y: py } = vp.world_to_pixel(corner).unwrap();
    let got = vp.pick_vertex(px, py);
    vp.set_edit_rest_mode(RestMode::Fade);
    assert!(got.is_none());
    vp.set_edit_rest_mode(RestMode::Hide);
    assert!(!vp
        .nearby_group_edges(px, py, 48.0, 48)
        .iter()
        .any(|e| (e.a - corner).length() < 1e-6 || (e.b - corner).length() < 1e-6));
    vp.set_edit_rest_mode(RestMode::Fade);
}

// ---- inside a container: box select, delete, everything -------------------

#[test]
fn test_delete_a_child_of_the_open_container_and_undo() {
    let mut vp = stub();
    let (p, h1, h2) = assembly(&mut vp.scene);
    vp.scene.begin_group_edit(p);
    vp.scene.selection.insert(EntityRef::Group(h1));
    vp.run(DeleteGroupCommand { group: h1 });
    assert!(vp.history.last_error.is_none());
    assert_eq!(kids(&vp.scene, p), vec![h2]);
    assert!(!vp.scene.selection.contains(&EntityRef::Group(h1)));
    assert!(vp.undo());
    assert_eq!(
        kids(&vp.scene, p),
        vec![h1, h2],
        "back in its place in the parent's list"
    );
    // and at the root as always
    vp.scene.end_group_edit();
    vp.run(DeleteGroupCommand { group: p });
    assert!(vp.scene.doc.groups.is_empty() && vp.undo());
    assert_eq!(
        vp.scene.doc.groups.iter().map(|g| g.id).collect::<Vec<_>>(),
        vec![p]
    );
}

#[test]
fn test_the_selection_box_inside_the_container_takes_its_children() {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x, p.y)));
    let (p, h1, h2) = assembly(&mut vp.scene);
    let outside = group(&mut vp.scene, square(20.0, 1.0), "Outside");
    let f = outside.id;
    vp.scene.doc.groups.push(outside);
    vp.scene.begin_group_edit(p);
    let set = |vp: &Viewport| -> std::collections::BTreeSet<GroupId> {
        vp.scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::Group(g) => Some(*g),
                _ => None,
            })
            .collect()
    };
    // a window box around the housing (0..1), not the outside one
    SelectTool::new().on_box_select(&mut vp, (-0.5, -0.5, 1.5, 1.5), false, SelectMode::Replace);
    assert_eq!(set(&vp), [h1].into_iter().collect());
    // a crossing box touching both children and the outside one too
    SelectTool::new().on_box_select(&mut vp, (0.5, 0.5, 25.0, 0.7), true, SelectMode::Replace);
    assert_eq!(
        set(&vp),
        [h1, h2].into_iter().collect(),
        "the rest does not get in"
    );
    vp.scene.end_group_edit();
    SelectTool::new().on_box_select(&mut vp, (0.5, 0.5, 25.0, 0.7), true, SelectMode::Replace);
    assert_eq!(
        set(&vp),
        [p, f].into_iter().collect(),
        "at the root, as always"
    );
}

#[test]
fn test_the_containers_box_follows_its_children() {
    let mut vp = stub();
    let (p, _h1, h2) = assembly(&mut vp.scene);
    vp.scene.doc.find_group_mut(p).unwrap().xform = Some(DMat4::from_translation(v3(10., 0., 0.))); // placed far away
    let bbox = |vp: &Viewport| {
        let (f, lo, hi) = vp.group_obb(p).unwrap();
        let xs: Vec<f64> = oriented_box_corners(f, lo, hi)
            .iter()
            .map(|c| c.x)
            .collect();
        let r = |x: f64| (x * 1e6).round() / 1e6;
        (
            r(xs.iter().cloned().fold(f64::MAX, f64::min)),
            r(xs.iter().cloned().fold(f64::MIN, f64::max)),
        )
    };
    assert_eq!(bbox(&vp), (10.0, 14.0)); // housing 0..1 and plate 3..4, +10
    vp.scene.begin_group_edit(p); // the matrix goes down to the children
    assert_eq!(bbox(&vp), (10.0, 14.0), "entering does not move the box");
    vp.scene
        .doc
        .find_group_mut(p)
        .unwrap()
        .children
        .retain(|c| c.id != h2);
    vp.scene.version += 1;
    assert_eq!(
        bbox(&vp),
        (10.0, 11.0),
        "without the plate, the box shrinks"
    );
}

/// The xs of the triangles drawn washed out (the context) and at full
/// light (the subject) in the next frame.
fn fade_split(vp: &mut Viewport) -> (Vec<i64>, Vec<i64>) {
    let f = vp.frame_data([200, 200], false);
    let mut faded = Vec::new();
    let mut lit = Vec::new();
    for d in f
        .draws
        .iter()
        .filter(|d| d.source == Source::SceneFaces && d.stipple == Stipple::None)
    {
        let tris = frame_triangles(&f, d.first, d.count);
        let out = if d.fade == EDIT_REST_FADE {
            &mut faded
        } else {
            &mut lit
        };
        for t in tris {
            out.push(t.iter().map(|c| c.pos.x).fold(f64::MAX, f64::min).round() as i64);
        }
    }
    for v in [&mut faded, &mut lit] {
        v.sort();
        v.dedup();
    }
    (faded, lit)
}

#[test]
fn test_instances_fade_too() {
    // Opening a group fades the rest of the model, instances included.
    // Xtrude CAD bakes every placement into the one face buffer, so the
    // check is what each draw holds.
    let mut vp = stub();
    let (p, _h1, _h2) = assembly(&mut vp.scene);
    let loose = group(&mut vp.scene, square(20.0, 1.0), "Other");
    vp.scene.doc.groups.push(loose);
    vp.scene.version += 1;
    vp.set_edit_rest_mode(RestMode::Fade);
    assert_eq!(
        fade_split(&mut vp),
        (vec![], vec![0, 3, 20]),
        "at the root nothing fades"
    );

    vp.scene.begin_group_edit(p);
    let (faded, lit) = fade_split(&mut vp);
    assert_eq!(faded, vec![20], "the outside, faded");
    assert_eq!(lit, vec![0, 3], "the inside, at full light");

    vp.set_edit_rest_mode(RestMode::Hide);
    assert_eq!(
        fade_split(&mut vp),
        (vec![], vec![0, 3]),
        "in hide mode the outside is not drawn at all"
    );
}
