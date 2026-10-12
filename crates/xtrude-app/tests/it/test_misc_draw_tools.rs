// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Sector, the arc radius suffix, Make Component, Make Independent, Flip and Freehand.

use crate::common;
use xtrude_core::Axis;

use common::{stub, v3, MetreDocument};
use glam::{DMat4, DVec3};
use xtrude_app::input::{Key, Modifiers};
use xtrude_app::tools::arc::{ArcTool, CenterArcTool};
use xtrude_app::tools::flip::FlipTool;
use xtrude_app::tools::freehand::FreehandTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_app::viewport::Viewport;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_model::commands::{
    mirror_matrix, FlipVerticesCommand, GroupToComponentCommand, MakeGroupCommand,
    MakeIndependentCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, Group};

fn ctx(x: f64, y: f64) -> ToolContext {
    let mut c = ToolContext::at(v3(x, y, 0.0));
    c.screen = (x * 100.0, y * 100.0);
    c
}

#[test]
fn test_sector_closes_the_wedge_with_a_face() {
    let mut vp = stub();
    let mut t = CenterArcTool::sector();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_click(&mut vp, &ctx(2., 0.));
    t.on_hover(&mut vp, &ctx(0., 2.));
    t.on_click(&mut vp, &ctx(0., 2.));
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1);
    let at_centre = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            a.length() < 1e-6 || b.length() < 1e-6
        })
        .count();
    assert_eq!(at_centre, 2);
    let area = m.face_area(m.face_ids()[0]);
    assert!((area - std::f64::consts::PI).abs() < 0.15);
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 0);
}

#[test]
fn test_arc_radius_suffix_parses_and_commits() {
    let _m = MetreDocument::new();
    assert_eq!(parse_value_buffer("2r", false), Some(Typed::Radius(2.0)));
    assert_eq!(parse_value_buffer("2.5r", false), Some(Typed::Radius(2.5)));
    assert_eq!(parse_value_buffer("r", false), None);
    let mut vp = stub();
    let mut t = ArcTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0.));
    t.on_click(&mut vp, &ctx(2., 0.));
    t.on_hover(&mut vp, &ctx(1., 1.));
    assert!(t.on_radius_value(&mut vp, 0.4));
    assert_eq!(vp.scene.mesh().edges.len(), 0);
    t.on_click(&mut vp, &ctx(0., 0.));
    let mut t2 = ArcTool::new();
    t2.on_activate(&mut vp);
    t2.on_click(&mut vp, &ctx(0., 0.));
    t2.on_click(&mut vp, &ctx(2., 0.));
    t2.on_hover(&mut vp, &ctx(1., 1.));
    assert!(t2.on_radius_value(&mut vp, 1.0));
    let m = vp.scene.mesh();
    assert!(m.edges.len() > 4);
    let top = m
        .edge_ids()
        .into_iter()
        .map(|e| {
            let (a, b) = m.edge_ab(e);
            a.y.max(b.y)
        })
        .fold(f64::MIN, f64::max);
    assert!((top - 1.0).abs() < 0.05);
}

#[test]
fn test_make_component_shares_definition_with_copies() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(2., 3., 0.),
            v3(4., 3., 0.),
            v3(4., 5., 0.),
            v3(2., 5., 0.),
        ],
        &[],
    );
    let cmd = MakeGroupCommand::new(vp.scene.mesh(), &[f], &[], true, Some("Column"));
    vp.run(cmd);
    assert_eq!(vp.scene.doc.groups.len(), 1);
    let inst = vp.scene.doc.groups[0].clone();
    assert_eq!(inst.name, "Column");
    let xf = inst.xform.expect("an instance");
    let m = vp.scene.doc.mesh(inst.mesh);
    let lx = m
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MAX, f64::min);
    let ly = m
        .verts
        .iter()
        .map(|(_, v)| v.position.y)
        .fold(f64::MAX, f64::min);
    assert!(lx == 0.0 && ly == 0.0);
    let w = xf.project_point(DVec3::ZERO);
    assert!((w.x - 2.0).abs() < 1e-9 && (w.y - 3.0).abs() < 1e-9);
    let mut g = inst.clone();
    let twin = copy_group(&mut vp.scene.doc.meshes, &mut g, None, false);
    assert_eq!(twin.mesh, inst.mesh);
    assert!(vp.undo());
    assert!(vp.scene.doc.groups.is_empty());
}

#[test]
fn test_make_independent_detaches_the_instance() {
    let mut vp = stub();
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
    let mut a = Group::new(pid, Some("Peg"));
    a.xform = Some(DMat4::IDENTITY);
    let aid = a.id;
    vp.scene.doc.groups.push(a);
    vp.run(MakeIndependentCommand { group: aid });
    let g = vp.scene.doc.find_group(aid).unwrap();
    assert!(g.xform.is_none() && g.mesh != pid);
    assert!(vp.undo());
    let g = vp.scene.doc.find_group(aid).unwrap();
    assert!(g.mesh == pid && g.xform.is_some());
}

#[test]
fn test_mirror_matrix_reflects_and_is_involutive() {
    let m = mirror_matrix(v3(1., 0., 0.), v3(1., 0., 0.));
    let p = m.project_point(v3(3., 2., 5.));
    assert!((p.x + 1.0).abs() < 1e-6 && (p.y - 2.0).abs() < 1e-6 && (p.z - 5.0).abs() < 1e-6);
    assert!((m.project_point(p) - v3(3., 2., 5.)).length() < 1e-6);
}

#[test]
fn test_flip_vertices_mirrors_and_keeps_faces_outward() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(1., 0., 0.),
            v3(3., 0., 0.),
            v3(3., 2., 0.),
            v3(1., 2., 0.),
        ],
        &[],
    );
    let n0 = vp.scene.mesh().face_normal(f);
    let pos = vp.scene.mesh().face_positions(f);
    vp.run(FlipVerticesCommand {
        src: pos,
        centre: v3(2., 1., 0.),
        axis: v3(1., 0., 0.),
        faces: vec![f],
    });
    let m = vp.scene.mesh();
    let f0 = m.face_ids()[0];
    let mut xs: Vec<i64> = m
        .face_positions(f0)
        .iter()
        .map(|p| p.x.round() as i64)
        .collect();
    xs.sort();
    assert_eq!(xs, vec![1, 1, 3, 3]);
    assert!(n0.dot(m.face_normal(f0)) > 0.99);
    assert!(vp.undo());
}

fn unit_square_group(vp: &mut Viewport) -> xtrude_model::ids::GroupId {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(1., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 1., 0.),
            v3(1., 1., 0.),
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
fn test_flip_tool_flips_group_and_copy_mode() {
    let mut vp = stub();
    let g = unit_square_group(&mut vp);
    vp.scene.selection.insert(EntityRef::Group(g));
    let mut t = FlipTool::new();
    t.on_activate(&mut vp);
    assert!(t.on_key(&mut vp, Key::Right, Modifiers::NONE));
    t.on_click(&mut vp, &ctx(0., 0.));
    let gm = vp.scene.doc.find_group(g).unwrap().mesh;
    let mut xs: Vec<i64> = vp
        .scene
        .doc
        .mesh(gm)
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 100.0).round() as i64)
        .collect();
    xs.sort();
    assert_eq!(xs, vec![100, 100, 200, 200]);
    assert!(vp.undo());
    assert!(t.on_key(&mut vp, Key::Control, Modifiers::NONE));
    assert_eq!(t.lock_axis, Some(Axis::X));
    t.on_click(&mut vp, &ctx(0., 0.));
    assert_eq!(vp.scene.doc.groups.len(), 2);
    assert!(!t.copy);
    assert!(vp.undo());
    assert_eq!(
        vp.scene.doc.groups.iter().map(|x| x.id).collect::<Vec<_>>(),
        vec![g]
    );
}

#[test]
fn test_freehand_stroke_lands_as_one_curve() {
    let mut vp = stub();
    let mut t = FreehandTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0.));
    for i in 1..30 {
        let x = i as f64 * 0.2;
        t.on_hover(&mut vp, &ctx(x, x.sin()));
    }
    t.on_release(&mut vp);
    let m = vp.scene.mesh();
    assert!(m.edges.len() >= 4);
    let ids: std::collections::BTreeSet<Option<i64>> =
        m.edge_ids().into_iter().map(|e| m.edge(e).curve).collect();
    assert!(ids.len() == 1 && !ids.contains(&None));
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().edges.len(), 0);
}

#[test]
fn test_freehand_closed_stroke_forms_a_face() {
    let mut vp = stub();
    let mut t = FreehandTool::new();
    t.on_activate(&mut vp);
    t.on_click(&mut vp, &ctx(0., 0.));
    for ang in (12..360).step_by(12) {
        let a = (ang as f64).to_radians();
        t.on_hover(&mut vp, &ctx(2.0 - 2.0 * a.cos(), 2.0 * a.sin()));
    }
    let mut last = ToolContext::at(v3(0.001, 0.001, 0.));
    last.screen = (0.4, 0.4);
    t.on_hover(&mut vp, &last);
    t.on_release(&mut vp);
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_group_converts_to_component_in_place() {
    let mut vp = stub();
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("Rail"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.run(GroupToComponentCommand {
        group: gid,
        name: Some("Knob".into()),
    });
    let g = vp.scene.doc.find_group(gid).unwrap();
    assert!(g.xform == Some(DMat4::IDENTITY));
    assert_eq!(g.mesh, mid);
    assert_eq!(g.name, "Knob");
    assert!(vp.undo());
    let g = vp.scene.doc.find_group(gid).unwrap();
    assert!(g.xform.is_none() && g.name == "Rail");
}
