// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A component's contents are shared by all its copies: editing inside one
//! copy of a container component reaches every copy, as one undo step.
//! Groups never share.

use crate::common;

use common::{v3, viewport};
use glam::DMat4;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_core::Mesh;
use xtrude_model::commands::{
    AddFaceCommand, DeleteGroupCommand, InsertGroupCommand, MakeGroupCommand, MoveGroupCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, world_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::pushpull::PushPull;

/// A unit square at height `z`, in its own mesh.
fn square_mesh(vp: &mut Viewport, z: f64) -> MeshId {
    let mut m = Mesh::new();
    m.add_face(
        &[v3(0., 0., z), v3(1., 0., z), v3(1., 1., z), v3(0., 1., z)],
        &[],
    );
    add_mesh(&mut vp.scene.doc.meshes, m)
}

fn placed(mesh: MeshId, name: &str, x: DMat4, component: bool) -> Group {
    let mut g = Group::new(mesh, Some(name));
    g.xform = Some(x);
    g.component = component;
    g
}

/// The copy placed at `x` of `src`, sharing what `copy_group` shares.
fn copy_at(vp: &mut Viewport, src: GroupId, x: DMat4) -> GroupId {
    let mut g = vp.scene.doc.find_group(src).unwrap().clone();
    let mut dup = copy_group(&mut vp.scene.doc.meshes, &mut g, None, false);
    dup.xform = Some(x);
    let id = dup.id;
    vp.scene.doc.groups.push(dup);
    vp.scene.version += 1;
    id
}

fn g(vp: &Viewport, id: GroupId) -> &Group {
    vp.scene.doc.find_group(id).unwrap()
}

fn faces(vp: &Viewport, id: GroupId) -> usize {
    vp.scene.doc.mesh(g(vp, id).mesh).faces.len()
}

/// Push/pull the open group's first face up by 1, as one undo step.
fn push_first_face(vp: &mut Viewport) {
    let target = vp.scene.active_mesh_id();
    let face = vp.scene.mesh().face_ids()[0];
    let mut pp = PushPull::new();
    pp.setup_plain(vp.scene.mesh(), target, face);
    pp.extrusion = 1.0;
    pp.commit(&mut vp.scene, &mut vp.history);
}

fn flashes(vp: &Viewport) -> Vec<String> {
    vp.outbox
        .events
        .iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// A container ("Bracket": a square of its own and a "Bolt" child) placed
/// twice as a component, or as a group: (first, second, its mesh).
fn container_twice(vp: &mut Viewport, component: bool) -> (GroupId, GroupId, MeshId) {
    let body = square_mesh(vp, 0.);
    let bolt = square_mesh(vp, 2.);
    let mut a = placed(body, "Bracket", DMat4::IDENTITY, component);
    a.children = vec![placed(bolt, "Bolt", DMat4::IDENTITY, true)];
    let ia = a.id;
    vp.scene.doc.groups.push(a);
    let turned =
        DMat4::from_translation(v3(10., 0., 0.)) * DMat4::from_rotation_z(90f64.to_radians());
    let ib = copy_at(vp, ia, turned);
    (ia, ib, body)
}

#[test]
fn test_editing_a_container_copy_reaches_every_copy_in_one_undo() {
    let mut vp = viewport();
    let (a, b, body) = container_twice(&mut vp, true);
    let (xb, kids_b) = (g(&vp, b).xform, g(&vp, b).children.clone());
    vp.begin_group_edit(b);
    push_first_face(&mut vp);
    vp.end_one_group_edit();
    assert!(vp.scene.edit_group().is_none());
    assert!(
        g(&vp, a).mesh == body && g(&vp, b).mesh == body,
        "both copies still share one mesh"
    );
    assert!(faces(&vp, a) > 1, "the pushed face reached the other copy");
    assert_eq!(g(&vp, b).xform, xb, "the copy is back on its matrix");
    assert_eq!(g(&vp, b).children, kids_b, "with its children as they were");
    let low_x = vp
        .scene
        .doc
        .mesh(body)
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MAX, f64::min);
    assert!(low_x.abs() < 1e-9, "the definition is in local coordinates");
    let top = vp
        .scene
        .doc
        .mesh(body)
        .verts
        .iter()
        .map(|(_, v)| v.position.z)
        .fold(f64::MIN, f64::max);
    assert!((top - 1.0).abs() < 1e-9, "pushed up by 1, not {top}");
    assert_eq!(vp.history.undo_len(), 1, "one undo step");
    assert!(vp.undo());
    assert_eq!((faces(&vp, a), faces(&vp, b)), (1, 1));
    assert!(g(&vp, b).mesh == body && g(&vp, b).xform == xb);
    assert!(vp.redo());
    assert!(faces(&vp, a) > 1 && g(&vp, b).mesh == body);
}

#[test]
fn test_the_status_counts_nested_copies() {
    let mut vp = viewport();
    let (a, _b, _body) = container_twice(&mut vp, true);
    let bolt = g(&vp, a).children[0].id;
    vp.begin_group_edit(a);
    vp.begin_group_edit(bolt);
    push_first_face(&mut vp);
    vp.end_one_group_edit();
    let said = flashes(&vp);
    assert!(
        said.iter()
            .any(|s| s.contains("'Bolt'") && s.contains("its 2 copies")),
        "{said:?}"
    );
}

#[test]
fn test_a_copied_group_with_children_keeps_its_own_faces() {
    let mut vp = viewport();
    let (a, b, body) = container_twice(&mut vp, false);
    assert_eq!(g(&vp, b).mesh, body, "copying the group kept the mesh id");
    vp.begin_group_edit(b);
    push_first_face(&mut vp);
    vp.end_group_edit();
    assert_eq!(faces(&vp, a), 1, "the other group is unchanged");
    assert!(faces(&vp, b) > 1);
}

#[test]
fn test_a_group_sharing_a_component_mesh_keeps_its_faces() {
    let mut vp = viewport();
    let (a, b, body) = container_twice(&mut vp, true);
    let group = placed(
        body,
        "Plate",
        DMat4::from_translation(v3(0., 9., 0.)),
        false,
    );
    let ig = group.id;
    vp.scene.doc.groups.push(group);
    vp.begin_group_edit(a);
    push_first_face(&mut vp);
    vp.end_group_edit();
    assert!(faces(&vp, b) > 1, "the copies share the edit");
    assert_eq!(
        vp.scene.doc.mesh(g(&vp, ig).mesh).faces.len(),
        1,
        "the group is not a copy"
    );
}

// ---- the children are shared --------------------------------------------------

fn turned_at(x: f64) -> DMat4 {
    DMat4::from_translation(v3(x, 0., 0.)) * DMat4::from_rotation_z(90f64.to_radians())
}

/// `Leg ▸ (Servo, Bracket)`, a component without faces of its own, placed
/// three times (the last one turned): the three legs.
fn three_legs(vp: &mut Viewport) -> [GroupId; 3] {
    legs(vp, Vec::new())
}

/// The three legs, each also holding `extra` after its servo and bracket.
fn legs(vp: &mut Viewport, extra: Vec<Group>) -> [GroupId; 3] {
    let own = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let servo = square_mesh(vp, 0.);
    let bracket = square_mesh(vp, 1.);
    let mut leg = placed(own, "Leg", DMat4::IDENTITY, true);
    leg.children = vec![
        placed(
            servo,
            "Servo",
            DMat4::from_translation(v3(0., 2., 0.)),
            true,
        ),
        placed(bracket, "Bracket", DMat4::IDENTITY, true),
    ];
    leg.children.extend(extra);
    let first = leg.id;
    vp.scene.doc.groups.push(leg);
    let second = copy_at(vp, first, DMat4::from_translation(v3(5., 0., 0.)));
    let third = copy_at(vp, first, turned_at(10.));
    [first, second, third]
}

fn child_names(vp: &Viewport, id: GroupId) -> Vec<String> {
    g(vp, id).children.iter().map(|c| c.name.clone()).collect()
}

fn child(vp: &Viewport, id: GroupId, name: &str) -> Group {
    g(vp, id)
        .children
        .iter()
        .find(|c| c.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("no {name}"))
}

fn delete_child(vp: &mut Viewport, id: GroupId, name: &str) {
    let gone = child(vp, id, name).id;
    assert!(vp.run(DeleteGroupCommand { group: gone }));
}

/// The placements' local matrices, rounded, in `group`'s own frame.
fn local_matrices(vp: &Viewport, id: GroupId) -> Vec<Vec<i64>> {
    g(vp, id)
        .children
        .iter()
        .map(|c| {
            c.xform
                .unwrap_or(DMat4::IDENTITY)
                .to_cols_array()
                .iter()
                .map(|x| (x * 1e6).round() as i64)
                .collect()
        })
        .collect()
}

#[test]
fn test_deleting_a_part_in_one_leg_deletes_it_in_every_leg() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    vp.begin_group_edit(legs[1]);
    delete_child(&mut vp, legs[1], "Servo");
    vp.end_one_group_edit();
    for leg in legs {
        assert_eq!(child_names(&vp, leg), ["Bracket"]);
    }
    let said = flashes(&vp);
    assert!(
        said.iter()
            .any(|s| s.contains("'Leg'") && s.contains("its 3 copies")),
        "{said:?}"
    );
    assert_eq!(vp.history.undo_len(), 1, "one undo step");
    assert!(vp.undo());
    for leg in legs {
        assert_eq!(child_names(&vp, leg), ["Servo", "Bracket"]);
    }
    assert_eq!(g(&vp, legs[2]).xform, Some(turned_at(10.)));
    assert!(vp.redo());
    for leg in legs {
        assert_eq!(child_names(&vp, leg), ["Bracket"]);
    }
}

#[test]
fn test_a_group_made_in_one_leg_appears_in_every_leg() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    vp.begin_group_edit(legs[2]);
    // Pasted into the turned leg, in world coordinates.
    let mut plate = Mesh::new();
    plate.add_face(
        &[
            v3(10., 3., 0.),
            v3(11., 3., 0.),
            v3(11., 4., 0.),
            v3(10., 4., 0.),
        ],
        &[],
    );
    let plate = Group::new(add_mesh(&mut vp.scene.doc.meshes, plate), Some("Plate"));
    assert!(vp.run(InsertGroupCommand::new(plate)));
    vp.end_group_edit();
    let plates: Vec<Group> = legs.iter().map(|&l| child(&vp, l, "Plate")).collect();
    let ids: std::collections::HashSet<GroupId> = plates.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 3, "each copy has its own GroupId");
    assert!(plates.iter().all(|p| p.mesh == plates[0].mesh), "one mesh");
    let m = local_matrices(&vp, legs[0]);
    assert_eq!(local_matrices(&vp, legs[1]), m, "same local place");
    assert_eq!(local_matrices(&vp, legs[2]), m, "same local place");
    // In the turned leg, the plate is where it was drawn.
    let drawn = world_mesh(&vp.scene.doc.meshes, g(&vp, legs[2]));
    assert!(drawn
        .verts
        .iter()
        .any(|(_, v)| (v.position - v3(11., 4., 0.)).length() < 1e-9));
}

#[test]
fn test_make_group_in_one_leg_nests_it_in_every_leg_in_one_undo() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    vp.begin_group_edit(legs[2]);
    // Drawn in the turned leg, in world coordinates, then grouped there.
    assert!(vp.run(AddFaceCommand::new(vec![
        v3(10., 3., 0.),
        v3(11., 3., 0.),
        v3(11., 4., 0.),
        v3(10., 4., 0.),
    ])));
    let m = vp.scene.mesh();
    let mut mk = MakeGroupCommand::new(m, &m.face_ids(), &[], false, Some("Plate"));
    assert!(vp.execute(&mut mk));
    assert_eq!(vp.scene.doc.groups.len(), 3, "nothing new at the top level");
    vp.end_group_edit();
    let plates: Vec<Group> = legs.iter().map(|&l| child(&vp, l, "Plate")).collect();
    let ids: std::collections::HashSet<GroupId> = plates.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 3, "each copy has its own GroupId");
    assert!(plates.iter().all(|p| p.mesh == plates[0].mesh), "one mesh");
    let m = local_matrices(&vp, legs[0]);
    assert_eq!(local_matrices(&vp, legs[1]), m, "same local place");
    assert_eq!(local_matrices(&vp, legs[2]), m, "same local place");
    let drawn = world_mesh(&vp.scene.doc.meshes, g(&vp, legs[2]));
    assert!(drawn
        .verts
        .iter()
        .any(|(_, v)| (v.position - v3(11., 4., 0.)).length() < 1e-9));
    assert_eq!(vp.history.undo_len(), 1, "one undo step");
    assert!(vp.undo());
    for leg in legs {
        assert_eq!(child_names(&vp, leg), ["Servo", "Bracket"]);
    }
}

#[test]
fn test_moving_a_part_in_a_turned_leg_moves_it_alike_in_every_leg() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    let before = child(&vp, legs[0], "Servo").xform.unwrap();
    vp.begin_group_edit(legs[2]);
    let servo = child(&vp, legs[2], "Servo").id;
    // One metre along world X is one metre along the turned leg's -Y.
    assert!(vp.run(MoveGroupCommand {
        group: servo,
        delta: v3(1., 0., 0.),
    }));
    vp.end_group_edit();
    for leg in legs {
        let moved = child(&vp, leg, "Servo").xform.unwrap();
        let shift = moved.w_axis.truncate() - before.w_axis.truncate();
        assert!((shift - v3(0., -1., 0.)).length() < 1e-9, "{shift}");
    }
}

#[test]
fn test_nested_copies_share_at_every_level() {
    // Body ▸ Leg×2 ▸ Servo ▸ Horn, and one more Servo on its own (the torso).
    let mut vp = viewport();
    let own = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let servo_mesh = square_mesh(&mut vp, 0.);
    let horn = square_mesh(&mut vp, 3.);
    let mut servo = placed(servo_mesh, "Servo", DMat4::IDENTITY, true);
    servo.children = vec![placed(horn, "Horn", DMat4::IDENTITY, true)];
    let torso_servo = {
        let mut t = servo.clone();
        let mut dup = copy_group(&mut vp.scene.doc.meshes, &mut t, None, false);
        dup.xform = Some(DMat4::from_translation(v3(0., 20., 0.)));
        dup
    };
    let mut leg = placed(own, "Leg", DMat4::IDENTITY, true);
    leg.children = vec![servo, placed(own, "Spacer", DMat4::IDENTITY, false)];
    let leg_a = leg.id;
    let leg_mesh = leg.mesh;
    let mut body = Group::new(
        add_mesh(&mut vp.scene.doc.meshes, Mesh::new()),
        Some("Body"),
    );
    body.xform = Some(DMat4::IDENTITY);
    body.component = false;
    let mut leg_b = {
        let mut l = leg.clone();
        copy_group(&mut vp.scene.doc.meshes, &mut l, None, false)
    };
    leg_b.xform = Some(turned_at(8.));
    let leg_b_id = leg_b.id;
    body.children = vec![leg, leg_b];
    let torso_id = torso_servo.id;
    vp.scene.doc.groups.extend([body, torso_servo]);
    vp.scene.version += 1;

    // Open a leg, then its servo; push the servo's face; leave the servo;
    // then delete the leg's spacer and leave the leg.
    vp.begin_group_edit(leg_a);
    let servo_a = child(&vp, leg_a, "Servo").id;
    vp.begin_group_edit(servo_a);
    push_first_face(&mut vp);
    vp.end_one_group_edit();
    delete_child(&mut vp, leg_a, "Spacer");
    vp.end_group_edit();

    let servos = [
        child(&vp, leg_a, "Servo"),
        child(&vp, leg_b_id, "Servo"),
        g(&vp, torso_id).clone(),
    ];
    for s in &servos {
        assert_eq!(s.mesh, servo_mesh, "every servo is still one definition");
        assert_eq!(child_names_of(s), ["Horn"]);
    }
    assert!(vp.scene.doc.mesh(servo_mesh).faces.len() > 1, "pushed");
    for leg in [leg_a, leg_b_id] {
        assert_eq!(g(&vp, leg).mesh, leg_mesh);
        assert_eq!(child_names(&vp, leg), ["Servo"], "the spacer went");
    }
    assert_eq!(vp.history.undo_len(), 1, "the whole session is one step");
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.mesh(servo_mesh).faces.len(), 1);
    for leg in [leg_a, leg_b_id] {
        assert_eq!(child_names(&vp, leg), ["Servo", "Spacer"]);
    }
}

fn child_names_of(g: &Group) -> Vec<String> {
    g.children.iter().map(|c| c.name.clone()).collect()
}

#[test]
fn test_a_look_inside_changes_nothing() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    let before: Vec<Group> = legs.iter().map(|&l| g(&vp, l).clone()).collect();
    vp.begin_group_edit(legs[2]);
    let servo = child(&vp, legs[2], "Servo").id;
    vp.begin_group_edit(servo);
    vp.end_group_edit();
    for (i, &l) in legs.iter().enumerate() {
        assert_eq!(g(&vp, l), &before[i], "leg {i} as it was");
    }
    assert_eq!(vp.history.undo_len(), 0);
}

#[test]
fn test_groups_holding_the_same_mesh_do_not_share_children() {
    let mut vp = viewport();
    let (a, b, body) = container_twice(&mut vp, false);
    assert_eq!(g(&vp, b).mesh, body);
    vp.begin_group_edit(b);
    delete_child(&mut vp, b, "Bolt");
    vp.end_group_edit();
    assert_eq!(child_names(&vp, a), ["Bolt"], "the other group keeps it");
    assert!(child_names(&vp, b).is_empty());
}

#[test]
fn test_a_group_and_a_component_holding_one_mesh_do_not_share_children() {
    let mut vp = viewport();
    let (a, b, body) = container_twice(&mut vp, true);
    let bolt = g(&vp, a).children[0].clone();
    let mut group = placed(
        body,
        "Plate",
        DMat4::from_translation(v3(0., 9., 0.)),
        false,
    );
    group.children = vec![copy_group(
        &mut vp.scene.doc.meshes,
        &mut bolt.clone(),
        None,
        true,
    )];
    let ig = group.id;
    vp.scene.doc.groups.push(group);
    // Edit the component: the group keeps its bolt.
    vp.begin_group_edit(a);
    delete_child(&mut vp, a, "Bolt");
    vp.end_group_edit();
    assert!(child_names(&vp, b).is_empty(), "the copy follows");
    assert_eq!(child_names(&vp, ig), ["Bolt"], "the group does not");
    // Edit the group: the components keep theirs.
    vp.begin_group_edit(ig);
    delete_child(&mut vp, ig, "Bolt");
    let face = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 5.),
            v3(1., 0., 5.),
            v3(1., 1., 5.),
            v3(0., 1., 5.),
        ],
        &[],
    );
    assert!(vp.scene.mesh().has_face(face));
    vp.end_group_edit();
    assert_eq!(faces(&vp, a), 1);
    assert_eq!(faces(&vp, b), 1);
}

#[test]
fn test_a_group_inside_a_component_is_shared_with_it() {
    // Leg ▸ (Frame ▸ Part): Frame is a group inside the component.
    let mut vp = viewport();
    let own = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let frame_mesh = square_mesh(&mut vp, 0.);
    let part = square_mesh(&mut vp, 1.);
    let mut frame = placed(frame_mesh, "Frame", DMat4::IDENTITY, false);
    frame.children = vec![placed(part, "Part", DMat4::IDENTITY, false)];
    let mut leg = placed(own, "Leg", DMat4::IDENTITY, true);
    leg.children = vec![frame];
    let first = leg.id;
    vp.scene.doc.groups.push(leg);
    let legs = [
        first,
        copy_at(&mut vp, first, DMat4::from_translation(v3(5., 0., 0.))),
        copy_at(&mut vp, first, turned_at(10.)),
    ];
    vp.begin_group_edit(legs[2]);
    let frame = child(&vp, legs[2], "Frame").id;
    vp.begin_group_edit(frame);
    delete_child(&mut vp, frame, "Part");
    vp.end_group_edit();
    for leg in legs {
        assert!(child(&vp, leg, "Frame").children.is_empty(), "Part is gone");
    }
    assert_eq!(vp.history.undo_len(), 1);
    assert!(vp.undo());
    for leg in legs {
        assert_eq!(child_names_of(&child(&vp, leg, "Frame")), ["Part"]);
    }
}

#[test]
fn test_shared_copies_stay_one_component_through_a_save() {
    let mut vp = viewport();
    let legs = three_legs(&mut vp);
    vp.begin_group_edit(legs[0]);
    delete_child(&mut vp, legs[0], "Bracket");
    vp.end_group_edit();
    let dir = std::env::temp_dir().join(format!("xtrude-legs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legs.xtr");
    xtrude_io::document::save_scene(&vp.scene, &path).unwrap();
    let mut back = xtrude_model::scene::Scene::new();
    xtrude_io::document::load_into(&mut back, &path, None).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let meshes: std::collections::HashSet<MeshId> =
        back.doc.groups.iter().map(|l| l.mesh).collect();
    assert_eq!(meshes.len(), 1, "the legs are still one component");
    let servos: std::collections::HashSet<MeshId> =
        back.doc.groups.iter().map(|l| l.children[0].mesh).collect();
    assert_eq!(servos.len(), 1);
}

#[test]
fn test_a_group_pasted_into_a_copy_without_children_reaches_every_copy() {
    let mut vp = viewport();
    let body = square_mesh(&mut vp, 0.);
    let first = placed(body, "Plate", DMat4::IDENTITY, true);
    let first_id = first.id;
    vp.scene.doc.groups.push(first);
    let turned = copy_at(&mut vp, first_id, turned_at(10.));
    vp.begin_group_edit(turned);
    // Pasted, and nothing else changed: the copy's own faces stay as they
    // were.
    let mut bolt = Mesh::new();
    bolt.add_face(
        &[
            v3(10., 3., 0.),
            v3(11., 3., 0.),
            v3(11., 4., 0.),
            v3(10., 4., 0.),
        ],
        &[],
    );
    let bolt = Group::new(add_mesh(&mut vp.scene.doc.meshes, bolt), Some("Bolt"));
    assert!(vp.run(InsertGroupCommand::new(bolt)));
    vp.end_group_edit();
    assert_eq!(
        child_names(&vp, first_id),
        ["Bolt"],
        "the other copy has it"
    );
    assert_eq!(local_matrices(&vp, first_id), local_matrices(&vp, turned));
    let drawn = world_mesh(&vp.scene.doc.meshes, g(&vp, turned));
    assert!(
        drawn
            .verts
            .iter()
            .any(|(_, v)| (v.position - v3(11., 4., 0.)).length() < 1e-9),
        "the bolt is where it was pasted"
    );
    assert_eq!(vp.history.undo_len(), 1, "one undo step");
    assert!(vp.undo());
    assert!(g(&vp, turned).children.is_empty() && g(&vp, first_id).children.is_empty());
}

// ---- Make Group two levels deep -------------------------------------------------

/// The three legs, each also holding a part ("Part": placed, turned, no
/// component of its own) and a group with children ("Frame ▸ Pin").
fn fuller_legs(vp: &mut Viewport) -> [GroupId; 3] {
    let part = square_mesh(vp, 0.);
    let frame = square_mesh(vp, 0.);
    let pin = square_mesh(vp, 0.);
    let mut frame = placed(
        frame,
        "Frame",
        DMat4::from_translation(v3(3., 0., 0.)),
        false,
    );
    frame.children = vec![placed(
        pin,
        "Pin",
        DMat4::from_translation(v3(0., 0., 1.)),
        false,
    )];
    let part = placed(
        part,
        "Part",
        DMat4::from_translation(v3(0., -3., 0.)) * DMat4::from_rotation_z(90f64.to_radians()),
        false,
    );
    legs(vp, vec![part, frame])
}

/// Whether a group's world geometry has a corner at `p`.
fn has_corner(vp: &Viewport, id: GroupId, p: glam::DVec3) -> bool {
    world_mesh(&vp.scene.doc.meshes, g(vp, id))
        .verts
        .iter()
        .any(|(_, v)| (v.position - p).length() < 1e-9)
}

/// Open the turned leg, then its `inner` part; draw a square there, high
/// above the rest, and run `action` (Make Group or Make Component, named
/// "Cap") on it; leave both levels. Every leg's `inner` holds the new
/// group, each its own, sharing one mesh at one local place; undo puts
/// every leg back. The new group, as the first leg holds it.
fn make_two_levels_deep(inner: &str, action: &str) -> Group {
    let mut w = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().texts.push(Some("Cap".into()));
    w.dialogs = Box::new(dialogs.clone());
    let legs = fuller_legs(&mut w.viewport);
    let vp = &mut w.viewport;
    let before: Vec<Group> = legs.iter().map(|&l| g(vp, l).clone()).collect();
    let kids = child(vp, legs[0], inner).children.len();
    vp.begin_group_edit(legs[2]);
    let open = child(vp, legs[2], inner).id;
    vp.begin_group_edit(open);
    // Drawn in world coordinates: in the turned leg, local (4, -1, 5).
    assert!(vp.run(AddFaceCommand::new(vec![
        v3(10., 3., 5.),
        v3(11., 3., 5.),
        v3(11., 4., 5.),
        v3(10., 4., 5.),
    ])));
    let mid = vp.scene.active_mesh_id();
    let m = vp.scene.doc.mesh(mid);
    let drawn: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.z > 4.))
        .collect();
    assert_eq!(drawn.len(), 1);
    vp.scene.selection.clear();
    vp.scene.selection.insert(EntityRef::Face(mid, drawn[0]));
    w.trigger(action);
    let vp = &mut w.viewport;
    vp.end_group_edit();
    assert!(vp.scene.edit_group().is_none());
    assert_eq!(vp.scene.doc.groups.len(), 3, "nothing new at the top level");
    let made: Vec<Group> = legs
        .iter()
        .map(|&l| {
            let c = child(vp, l, inner);
            assert_eq!(
                vp.scene.doc.mesh(c.mesh).faces.len(),
                1,
                "{inner} kept its face"
            );
            assert_eq!(c.children.len(), kids + 1, "{inner} holds the new group");
            c.children[kids].clone()
        })
        .collect();
    let ids: std::collections::HashSet<GroupId> = made.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 3, "each copy has its own GroupId");
    assert!(made.iter().all(|p| p.mesh == made[0].mesh), "one mesh");
    assert!(
        made.iter().all(|p| p.xform == made[0].xform),
        "one local place"
    );
    assert_eq!(vp.scene.doc.mesh(made[0].mesh).faces.len(), 1);
    assert!(
        has_corner(vp, legs[2], v3(11., 4., 5.)),
        "where it was drawn"
    );
    assert!(has_corner(vp, legs[0], v3(4., -1., 5.)));
    assert!(has_corner(vp, legs[1], v3(9., -1., 5.)));
    assert_eq!(vp.history.undo_len(), 1, "one undo step");
    assert!(vp.undo());
    for (i, &l) in legs.iter().enumerate() {
        assert_eq!(g(vp, l), &before[i], "leg {i} as it was");
    }
    made[0].clone()
}

#[test]
fn test_make_group_in_a_nested_component_of_a_leg_reaches_every_leg() {
    let made = make_two_levels_deep("Bracket", "make_group");
    assert!(!made.is_component());
}

#[test]
fn test_make_group_in_a_part_of_a_leg_reaches_every_leg() {
    let made = make_two_levels_deep("Part", "make_group");
    assert!(!made.is_component());
}

#[test]
fn test_make_group_in_a_group_with_children_in_a_leg_reaches_every_leg() {
    let made = make_two_levels_deep("Frame", "make_group");
    assert!(!made.is_component());
}

#[test]
fn test_make_component_two_levels_deep_in_a_leg_reaches_every_leg() {
    let made = make_two_levels_deep("Bracket", "make_component");
    assert!(made.is_component() && made.name == "Cap");
}
