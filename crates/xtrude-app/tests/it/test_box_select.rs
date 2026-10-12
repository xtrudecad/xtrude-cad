// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rubber-band selection, window (only what is entirely inside) vs crossing
//! (anything touched), groups, instances, guides and the group-edit context.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DMat4;
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_core::{EdgeId, FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::guide::Guide;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::SelectMode;

const RECT: (f64, f64, f64, f64) = (0.0, 0.0, 5.0, 5.0);

/// World (x, y) straight to pixels.
fn plan() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p| Some((p.x, p.y)));
    vp
}

fn boxsel(vp: &mut Viewport, crossing: bool, additive: bool) {
    let mode = if additive {
        SelectMode::Add
    } else {
        SelectMode::Replace
    };
    SelectTool::new().on_box_select(vp, RECT, crossing, mode);
}

struct Bits {
    edges: [EdgeId; 3],
    faces: [FaceId; 2],
}

fn scene(vp: &mut Viewport) -> Bits {
    let m = vp.scene.mesh_mut();
    let e0 = m.add_edge(v3(1., 1., 0.), v3(2., 2., 0.)).unwrap(); // fully inside
    let e1 = m.add_edge(v3(4., 4., 0.), v3(10., 10., 0.)).unwrap(); // crosses out
    let e2 = m.add_edge(v3(20., 20., 0.), v3(30., 30., 0.)).unwrap(); // outside
    let f0 = m.add_face(
        &[
            v3(1., 1., 0.),
            v3(2., 1., 0.),
            v3(2., 2., 0.),
            v3(1., 2., 0.),
        ],
        &[],
    );
    let f1 = m.add_face(
        &[
            v3(4., 4., 0.),
            v3(10., 4., 0.),
            v3(10., 10., 0.),
            v3(4., 10., 0.),
        ],
        &[],
    );
    Bits {
        edges: [e0, e1, e2],
        faces: [f0, f1],
    }
}

fn has(vp: &Viewport, e: EntityRef) -> bool {
    vp.scene.selection.contains(&e)
}

fn e(x: EdgeId) -> EntityRef {
    EntityRef::Edge(MeshId::LOOSE, x)
}

fn f(x: FaceId) -> EntityRef {
    EntityRef::Face(MeshId::LOOSE, x)
}

#[test]
fn test_window_selects_only_fully_enclosed() {
    let mut vp = plan();
    let b = scene(&mut vp);
    boxsel(&mut vp, false, false);
    assert!(has(&vp, e(b.edges[0])));
    assert!(!has(&vp, e(b.edges[1])));
    assert!(!has(&vp, e(b.edges[2])));
    assert!(has(&vp, f(b.faces[0])));
    assert!(!has(&vp, f(b.faces[1])));
}

#[test]
fn test_crossing_selects_anything_touched() {
    let mut vp = plan();
    let b = scene(&mut vp);
    boxsel(&mut vp, true, false);
    assert!(has(&vp, e(b.edges[0])));
    assert!(has(&vp, e(b.edges[1])));
    assert!(!has(&vp, e(b.edges[2])));
    assert!(has(&vp, f(b.faces[0])));
    assert!(has(&vp, f(b.faces[1])));
}

#[test]
fn test_box_select_additive_keeps_previous() {
    let mut vp = plan();
    let b = scene(&mut vp);
    vp.scene.select([e(b.edges[2])], SelectMode::Replace);
    boxsel(&mut vp, false, true);
    assert!(has(&vp, e(b.edges[2])));
    assert!(has(&vp, e(b.edges[0])));
}

fn group_at(vp: &mut Viewport, x0: f64, y0: f64) -> GroupId {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(x0, y0, 0.),
            v3(x0 + 2., y0, 0.),
            v3(x0 + 2., y0 + 2., 0.),
            v3(x0, y0 + 2., 0.),
        ],
        &[],
    );
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(id, None);
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    gid
}

#[test]
fn test_window_box_selects_groups_fully_inside() {
    let mut vp = plan();
    let (a, b, c) = (
        group_at(&mut vp, 1., 1.),
        group_at(&mut vp, 4., 4.),
        group_at(&mut vp, 20., 20.),
    );
    boxsel(&mut vp, false, false);
    assert!(has(&vp, EntityRef::Group(a)));
    assert!(!has(&vp, EntityRef::Group(b)));
    assert!(!has(&vp, EntityRef::Group(c)));
}

#[test]
fn test_crossing_box_selects_touched_groups() {
    let mut vp = plan();
    let (a, b, c) = (
        group_at(&mut vp, 1., 1.),
        group_at(&mut vp, 4., 4.),
        group_at(&mut vp, 20., 20.),
    );
    boxsel(&mut vp, true, false);
    assert!(has(&vp, EntityRef::Group(a)));
    assert!(has(&vp, EntityRef::Group(b)));
    assert!(!has(&vp, EntityRef::Group(c)));
}

#[test]
fn test_crossing_box_catches_group_edge_straddling_it() {
    let mut vp = plan();
    let mut m = Mesh::new();
    m.add_edge(v3(-10., 2.5, 0.), v3(10., 2.5, 0.)).unwrap();
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(id, None);
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    boxsel(&mut vp, true, false);
    assert!(has(&vp, EntityRef::Group(gid)));
}

#[test]
fn test_box_select_maps_instances_to_world() {
    let mut vp = plan();
    let mut proto = Mesh::new();
    proto.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let pid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let inst = |vp: &mut Viewport, dx: f64, dy: f64| {
        let mut g = Group::new(pid, None);
        g.xform = Some(DMat4::from_translation(v3(dx, dy, 0.)));
        let id = g.id;
        vp.scene.doc.groups.push(g);
        id
    };
    let near = inst(&mut vp, 1., 1.);
    let far = inst(&mut vp, 20., 20.);
    boxsel(&mut vp, false, false);
    assert!(has(&vp, EntityRef::Group(near)));
    assert!(!has(&vp, EntityRef::Group(far)));
}

#[test]
fn test_crossing_box_selects_guides_window_does_not() {
    let mut vp = plan();
    let line = Guide::new(v3(2., 2., 0.), Some(v3(1., 0., 0.)), None);
    let point = Guide::new(v3(3., 3., 0.), None, None);
    let far = Guide::new(v3(50., 50., 0.), None, None);
    let (l, p, fp) = (line.id, point.id, far.id);
    vp.scene.doc.guides.extend([line, point, far]);
    boxsel(&mut vp, false, false);
    assert!(!has(&vp, EntityRef::Guide(l))); // window: never the line
    assert!(has(&vp, EntityRef::Guide(p)));
    assert!(!has(&vp, EntityRef::Guide(fp)));
    boxsel(&mut vp, true, false);
    assert!(has(&vp, EntityRef::Guide(l))); // crossing takes it
    assert!(has(&vp, EntityRef::Guide(p)));
}

#[test]
fn test_delete_key_erases_selected_guides() {
    let mut vp = plan();
    let g = Guide::new(v3(0., 0., 0.), Some(v3(1., 0., 0.)), None);
    let gid = g.id;
    vp.scene.doc.guides.push(g.clone());
    vp.scene.selection.insert(EntityRef::Guide(gid));
    assert!(SelectTool::new().on_key(&mut vp, Key::Delete, Modifiers::NONE));
    assert!(vp.scene.doc.guides.is_empty());
    assert!(!has(&vp, EntityRef::Guide(gid)));
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.guides, vec![g]);
}

#[test]
fn test_box_ignores_other_groups_inside_group_edit() {
    let mut vp = plan();
    let edited = group_at(&mut vp, 30., 30.);
    let other = group_at(&mut vp, 1., 1.);
    vp.scene.begin_group_edit(edited);
    boxsel(&mut vp, true, false);
    assert!(!has(&vp, EntityRef::Group(other)));
    vp.scene.end_group_edit();
}
