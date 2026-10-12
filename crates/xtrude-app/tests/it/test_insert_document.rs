// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Placing an inserted document: it is placed by composing its matrix (the
//! meshes never move), and hangs from its origin when the caller says so.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::place_group::PlaceGroupTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, iter_placements, placement_points, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::insert::component_from_doc;
use xtrude_model::scene::Doc;

fn square(mut mesh: Mesh, x0: f64, side: f64, z: f64) -> Mesh {
    mesh.add_face(
        &[
            v3(x0, 0., z),
            v3(x0 + side, 0., z),
            v3(x0 + side, side, z),
            v3(x0, side, z),
        ],
        &[],
    );
    mesh
}

fn bracket_doc() -> Doc {
    let mut src = Doc::default();
    let loose = square(Mesh::new(), 0.0, 3.0, 0.0);
    src.meshes.insert(MeshId::LOOSE, std::sync::Arc::new(loose));
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 0.2, 0.0));
    src.groups.push(Group::new(id, Some("Peg")));
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 3.0, 2.4));
    let mut lid = Group::new(id, Some("Lid"));
    lid.xform = Some(DMat4::IDENTITY);
    src.groups.push(lid);
    src
}

fn positions(vp: &xtrude_app::viewport::Viewport, g: &Group) -> Vec<Vec<DVec3>> {
    iter_placements(g, None)
        .iter()
        .map(|(p, _)| {
            let m = vp.scene.doc.mesh(p.mesh);
            m.verts.iter().map(|(_, v)| v.position).collect()
        })
        .collect()
}

#[test]
fn test_placing_a_container_composes_its_matrix_and_leaves_the_meshes_alone() {
    let mut vp = stub();
    let comp = component_from_doc(bracket_doc(), &mut vp.scene.doc.meshes, "bracket").unwrap();
    let id = comp.id;
    let before = positions(&vp, &comp);
    let mut tool = PlaceGroupTool::new(&vp.scene.doc.meshes, comp, false, None);
    // the anchor is the centre of the base of the WHOLE component (3×3 at z=0)
    assert!((tool.anchor - v3(1.5, 1.5, 0.)).length() < 1e-6);
    tool.on_click(&mut vp, &ToolContext::at(v3(10., 20., 0.)));
    assert!(vp.scene.doc.groups.iter().any(|g| g.id == id));
    assert!(vp.scene.selection.contains(&EntityRef::Group(id)));
    let comp = vp.scene.doc.find_group(id).unwrap().clone();
    assert_eq!(
        positions(&vp, &comp),
        before,
        "neither its own mesh nor the children's move"
    );
    let pts = placement_points(&vp.scene.doc.meshes, &comp);
    let lo = pts
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    assert!(
        (lo.x - 8.5).abs() < 1e-6 && (lo.y - 18.5).abs() < 1e-6,
        "the whole component moved with the matrix"
    );
    assert!(vp.undo() && !vp.scene.doc.groups.iter().any(|g| g.id == id));
}

#[test]
fn test_an_inserted_document_hangs_from_its_origin() {
    let mut vp = stub();
    let mut src = Doc::default();
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 1.0, -1.3));
    src.groups.push(Group::new(id, Some("Foot")));
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 1.0, 0.0));
    src.groups.push(Group::new(id, Some("Plate")));
    let comp = component_from_doc(src, &mut vp.scene.doc.meshes, "hinge").unwrap();
    let cid = comp.id;
    let mut tool = PlaceGroupTool::new(&vp.scene.doc.meshes, comp, false, Some(v3(0., 0., 0.)));
    assert!(tool.anchor.length() < 1e-9);
    tool.on_click(&mut vp, &ToolContext::at(v3(10., 10., 0.)));
    let comp = vp.scene.doc.find_group(cid).unwrap();
    let pts = placement_points(&vp.scene.doc.meshes, comp);
    let lo = pts
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    assert!((lo.z - (-1.3)).abs() < 1e-6, "the footing stays buried");
    assert!((lo.x - 10.0).abs() < 1e-6);
}
