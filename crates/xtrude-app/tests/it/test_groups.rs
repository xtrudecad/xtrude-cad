// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Groups isolate geometry into their own mesh so it does not weld to the
//! rest; soft/curve flags and paint travel in and out; Merge Groups fuses
//! without touching the loose mesh.

use crate::common;

use common::{stub, v3, TmpDir};
use glam::{DMat4, DVec3};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::{FaceId, Mesh};
use xtrude_io::document;
use xtrude_model::commands::{
    ExplodeGroupCommand, MakeGroupCommand, MergeGroupsCommand, MoveGroupCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::Scene;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn two_squares() -> (Viewport, FaceId) {
    let mut vp = stub();
    let m = vp.scene.mesh_mut();
    let a = m.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    m.add_face(&[v(5., 0.), v(7., 0.), v(7., 2.), v(5., 2.)], &[]);
    (vp, a)
}

fn make_group(vp: &mut Viewport, faces: &[FaceId], edges: &[xtrude_core::EdgeId]) -> GroupId {
    let mut c = MakeGroupCommand::new(vp.scene.mesh(), faces, edges, false, None);
    assert!(vp.execute(&mut c));
    c.group.unwrap()
}

fn group_mesh(vp: &Viewport, g: GroupId) -> &Mesh {
    let mid = vp.scene.doc.groups.iter().find(|q| q.id == g).unwrap().mesh;
    vp.scene.doc.mesh(mid)
}

fn xs(m: &Mesh) -> Vec<i64> {
    let mut out: Vec<i64> = m
        .verts
        .iter()
        .map(|(_, v)| v.position.x.round() as i64)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn test_make_group_moves_geometry_off_the_loose_mesh() {
    let (mut vp, a) = two_squares();
    let g = make_group(&mut vp, &[a], &[]);
    assert_eq!(vp.scene.doc.groups.len(), 1);
    assert_eq!(vp.scene.mesh().face_ids().len(), 1); // only the other square
    assert_eq!(group_mesh(&vp, g).face_ids().len(), 1);
    assert!(vp.scene.selection.contains(&EntityRef::Group(g))); // selected
    assert!(vp.undo());
    assert!(vp.scene.doc.groups.is_empty());
    assert_eq!(vp.scene.mesh().face_ids().len(), 2);
}

#[test]
fn test_moving_a_group_does_not_drag_loose_geometry() {
    let (mut vp, a) = two_squares();
    let g = make_group(&mut vp, &[a], &[]);
    let loose_before = xs(vp.scene.mesh());
    vp.execute(&mut MoveGroupCommand {
        group: g,
        delta: v(10., 0.),
    });
    // The group moved (its geometry, or its placement), the loose square not.
    let world_xs = |vp: &Viewport| {
        let grp = vp.scene.doc.groups.iter().find(|q| q.id == g).unwrap();
        let x = grp.xform.unwrap_or(DMat4::IDENTITY);
        let mut out: Vec<i64> = vp
            .scene
            .doc
            .mesh(grp.mesh)
            .verts
            .iter()
            .map(|(_, p)| x.transform_point3(p.position).x.round() as i64)
            .collect();
        out.sort();
        out.dedup();
        out
    };
    assert_eq!(world_xs(&vp), vec![10, 12]);
    assert_eq!(xs(vp.scene.mesh()), loose_before);
    assert!(vp.undo());
    assert_eq!(world_xs(&vp), vec![0, 2]);
}

#[test]
fn test_explode_merges_group_back_into_loose_mesh() {
    let (mut vp, a) = two_squares();
    let g = make_group(&mut vp, &[a], &[]);
    vp.execute(&mut ExplodeGroupCommand { group: g });
    assert!(vp.scene.doc.groups.is_empty());
    assert_eq!(vp.scene.mesh().face_ids().len(), 2); // back among the loose
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.groups.len(), 1);
}

fn soft_curve(m: &Mesh) -> (usize, usize) {
    let soft = m.edges.iter().filter(|(_, e)| e.soft).count();
    let curve = m.edges.iter().filter(|(_, e)| e.curve.is_some()).count();
    (soft, curve)
}

#[test]
fn test_group_keeps_soft_and_curve_flags_round_trip() {
    // Grouping a smooth cylinder must not expose its facet seams.
    let mut vp = stub();
    let mut t = RadialTool::circle();
    for x in [0., 3.] {
        t.on_click(&mut vp, &ToolContext::at(v(x, 0.)));
    }
    let base = vp.scene.mesh().face_ids()[0];
    let mut pp = PushPullTool::new();
    pp.begin(&mut vp, MeshId::LOOSE, base);
    pp.pp.extrusion = 2.0;
    pp.commit(&mut vp);
    let before = soft_curve(vp.scene.mesh());
    assert_eq!(before.0, 24); // the cylinder's facet seams

    let faces = vp.scene.mesh().face_ids();
    let edges = vp.scene.mesh().edge_ids();
    let g = make_group(&mut vp, &faces, &edges);
    assert_eq!(soft_curve(group_mesh(&vp, g)), before);

    // document round-trip keeps the group smooth.
    let dir = TmpDir::new();
    let p = dir.path().join("cyl.xtrude");
    document::save_scene(&vp.scene, &p).unwrap();
    let mut scene2 = Scene::new();
    document::load_into(&mut scene2, &p, None).unwrap();
    let g2 = &scene2.doc.groups[0];
    assert_eq!(soft_curve(scene2.doc.mesh(g2.mesh)).0, before.0);

    // Explode: flags travel back to the loose mesh.
    vp.execute(&mut ExplodeGroupCommand { group: g });
    assert_eq!(soft_curve(vp.scene.mesh()), before);
}

#[test]
fn test_group_keeps_face_attrs_round_trip() {
    // Colour and material painted on faces survive Make Group and travel
    // back out on Explode.
    let (mut vp, a) = two_squares();
    let red = [0.8, 0.2, 0.1];
    {
        let attrs = &mut vp.scene.mesh_mut().face_mut(a).attrs;
        attrs.paint.color = Some(red);
        attrs.paint.mat = Some("Brick red".into());
    }
    let g = make_group(&mut vp, &[a], &[]);
    let gm = group_mesh(&vp, g);
    let gf = gm.face(gm.face_ids()[0]);
    assert_eq!(gf.attrs.paint.color, Some(red));
    assert_eq!(gf.attrs.paint.mat.as_deref(), Some("Brick red"));
    vp.execute(&mut ExplodeGroupCommand { group: g });
    let m = vp.scene.mesh();
    let back = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face(f).attrs.paint.color == Some(red))
        .expect("the painted face");
    assert_eq!(m.face(back).attrs.paint.mat.as_deref(), Some("Brick red"));
}

#[test]
fn test_merge_groups_fuses_without_touching_loose_mesh() {
    let mut vp = stub();
    let mut m1 = Mesh::new();
    let f = m1.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    m1.face_mut(f).attrs.paint.color = Some([1.0, 0.0, 0.0]);
    let e = m1.add_edge(v3(0., 0., 0.), v3(0., 0., 2.)).unwrap();
    m1.edge_mut(e).soft = true;
    let mid1 = add_mesh(&mut vp.scene.doc.meshes, m1);
    let g1 = Group::new(mid1, Some("Base"));
    let mut proto = Mesh::new();
    proto.add_face(&[v3(0., 0., 0.), v3(1., 0., 0.), v3(0., 1., 0.)], &[]);
    let mid2 = add_mesh(&mut vp.scene.doc.meshes, proto);
    let mut g2 = Group::new(mid2, Some("Leaf"));
    g2.xform = Some(DMat4::from_translation(v3(5., 0., 0.)));
    let ids = vec![g1.id, g2.id];
    vp.scene.doc.groups.extend([g1, g2]);
    vp.scene.version += 1;
    let loose0 = vp.scene.mesh().face_ids().len();

    let mut c = MergeGroupsCommand {
        groups: ids.clone(),
        merged: None,
    };
    vp.execute(&mut c);
    assert_eq!(vp.scene.doc.groups.len(), 1);
    let merged = &vp.scene.doc.groups[0];
    assert_eq!(merged.name, "Base");
    let mm = vp.scene.doc.mesh(merged.mesh);
    assert_eq!(mm.face_ids().len(), 2);
    assert_eq!(vp.scene.mesh().face_ids().len(), loose0); // loose untouched
    assert!(mm
        .face_ids()
        .iter()
        .any(|&f| mm.face(f).attrs.paint.color == Some([1.0, 0.0, 0.0])));
    let tri = mm
        .face_ids()
        .into_iter()
        .find(|&f| mm.face(f).outer.len() == 3)
        .unwrap();
    let mx = merged.xform.unwrap_or(DMat4::IDENTITY);
    assert!(mm
        .face_positions(tri)
        .iter()
        .all(|p| mx.transform_point3(*p).x >= 5.0 - 1e-9)); // baked to world
    assert!(mm.edges.iter().any(|(_, e)| e.soft));
    assert!(vp.scene.selection.contains(&EntityRef::Group(merged.id)));

    assert!(vp.undo());
    assert_eq!(
        vp.scene.doc.groups.iter().map(|g| g.id).collect::<Vec<_>>(),
        ids
    );
    assert!(vp.redo() && vp.scene.doc.groups.len() == 1);
}
