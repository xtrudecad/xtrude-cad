// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Document tests — another .igz document inserted as ONE component. (The
//! saved-file tests live in xtrude-io, the placement tests with the app's
//! PlaceGroupTool.)

use crate::common;
use common::*;
use xtrude_core::attrs::{Back, Paint};
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group, MeshTable};
use xtrude_model::ids::MeshId;
use xtrude_model::insert::{component_from_doc, ensure_layers, merge_materials};
use xtrude_model::layers::Layer;
use xtrude_model::materials::Material;
use xtrude_model::scene::Doc;

fn square(mut mesh: Mesh, x0: f64, side: f64, z: f64) -> Mesh {
    mesh.add_face(
        &[
            V3(x0, 0., z),
            V3(x0 + side, 0., z),
            V3(x0 + side, side, z),
            V3(x0, side, z),
        ],
        &[],
    );
    mesh
}

/// A part document: loose geometry + two groups, one an instance.
fn bracket_doc() -> (Doc, Group, Group) {
    let mut src = Doc::default();
    let loose = square(Mesh::new(), 0.0, 3.0, 0.0);
    src.meshes.insert(MeshId::LOOSE, std::sync::Arc::new(loose));
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 0.2, 0.0));
    let peg = Group::new(id, Some("Peg"));
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 3.0, 2.4));
    let mut lid = Group::new(id, Some("Lid"));
    lid.xform = Some(DMat4::IDENTITY);
    src.groups.push(peg.clone());
    src.groups.push(lid.clone());
    (src, peg, lid)
}

#[test]
fn test_a_whole_document_is_a_container_with_its_groups_as_children() {
    let (src, peg, lid) = bracket_doc();
    let mut t = MeshTable::new();
    let comp = component_from_doc(src, &mut t, "bracket").unwrap();
    assert_eq!(comp.name, "bracket");
    assert_eq!(
        t[&comp.mesh].faces.len(),
        1,
        "the loose geometry is its own mesh"
    );
    assert_eq!(comp.children, vec![peg, lid], "the groups, its children");
    assert!(comp.xform.is_some(), "a container is always an instance");
    assert!(comp.children[1].xform.is_some() && comp.children[0].xform.is_none());
}

#[test]
fn test_a_document_that_is_one_group_comes_in_as_that_group() {
    let mut src = Doc::default();
    let id = add_mesh(&mut src.meshes, square(Mesh::new(), 0.0, 1.0, 0.0));
    let hinge = Group::new(id, None); // called "Group N"
    let hinge_id = hinge.id;
    src.groups.push(hinge);
    let mut t = MeshTable::new();
    let comp = component_from_doc(src, &mut t, "hinge").unwrap();
    assert_eq!(comp.id, hinge_id);
    assert_eq!(
        comp.name, "hinge",
        "an automatic name gives way to the file's"
    );

    let mut src2 = Doc::default();
    let id = add_mesh(&mut src2.meshes, square(Mesh::new(), 0.0, 1.0, 0.0));
    src2.groups.push(Group::new(id, Some("Printed hinge")));
    assert_eq!(
        component_from_doc(src2, &mut t, "hinge").unwrap().name,
        "Printed hinge"
    );
}

#[test]
fn test_only_loose_geometry_comes_in_as_a_plain_group_and_nothing_is_nothing() {
    let mut src = Doc::default();
    src.mesh_mut(MeshId::LOOSE).add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 1., 0.),
            V3(0., 1., 0.),
        ],
        &[],
    );
    let mut t = MeshTable::new();
    let comp = component_from_doc(src, &mut t, "spacer").unwrap();
    assert_eq!(t[&comp.mesh].faces.len(), 1);
    assert!(comp.children.is_empty() && comp.xform.is_none());
    assert!(component_from_doc(Doc::default(), &mut t, "empty").is_none());
}

#[test]
fn test_materials_cross_by_name_and_a_clash_is_renamed() {
    let (mut src, peg, lid) = bracket_doc();
    let set = |src: &mut Doc, g: &Group, f: &dyn Fn(&mut xtrude_core::attrs::FaceAttrs)| {
        let m = src.mesh_mut(g.mesh);
        let fid = m.face_ids()[0];
        f(&mut m.face_mut(fid).attrs);
    };
    set(&mut src, &peg, &|a| {
        a.paint.mat = Some("PLA".into());
        a.paint.color = Some([0.5, 0.3, 0.1]);
    });
    set(&mut src, &lid, &|a| {
        a.paint.mat = Some("PETG".into());
        a.back = Some(Back::Own(Paint {
            color: Some([0., 0., 1.]),
            mat: Some("PETG".into()),
            opacity: None,
        }));
    });
    src.materials.insert(
        "PLA".into(),
        Material::new("PLA", Some([0.5, 0.3, 0.1]), None),
    );
    src.materials.insert(
        "PETG".into(),
        Material::new("PETG", Some([0.0, 0.0, 1.0]), Some(0.4)),
    );
    let mut dst = Doc::default();
    dst.materials.insert(
        "PLA".into(),
        Material::new("PLA", Some([0.5, 0.3, 0.1]), None),
    );
    dst.materials.insert(
        "PETG".into(),
        Material::new("PETG", Some([0.9, 0.9, 0.9]), None),
    );
    let src_materials = src.materials.clone();
    let comp = component_from_doc(src, &mut dst.meshes, "bracket").unwrap();
    let renamed = merge_materials(&src_materials, &mut dst.materials, &comp, &mut dst.meshes);
    assert_eq!(
        renamed.into_iter().collect::<Vec<_>>(),
        [("PETG".to_string(), "PETG (2)".to_string())],
        "same recipe = same; different = (2)"
    );
    assert_eq!(dst.materials["PETG (2)"].opacity, Some(0.4));
    let attrs = |g: &Group| {
        let m = dst.mesh(g.mesh);
        m.face(m.face_ids()[0]).attrs.clone()
    };
    assert_eq!(attrs(&peg).paint.mat.as_deref(), Some("PLA"));
    let t = attrs(&lid);
    assert_eq!(t.paint.mat.as_deref(), Some("PETG (2)"));
    match t.back {
        Some(Back::Own(p)) => assert_eq!(p.mat.as_deref(), Some("PETG (2)")),
        other => panic!("back lost: {other:?}"),
    }
}

#[test]
fn test_missing_layers_are_created_visible() {
    let (mut src, peg, lid) = bracket_doc();
    src.groups[0].layer = Some("Fixtures".into());
    let m = src.mesh_mut(lid.mesh);
    let fid = m.face_ids()[0];
    m.face_mut(fid).attrs.layer = Some("Covers".into());
    let _ = peg;
    let mut dst = Doc::default();
    let mut hidden = Layer::new("Fixtures");
    hidden.visible = false;
    dst.layers.push(hidden);
    let mut t = MeshTable::new();
    let comp = component_from_doc(src, &mut t, "bracket").unwrap();
    let added = ensure_layers(&mut dst, &comp, &t);
    assert_eq!(added, ["Covers"]);
    assert!(dst.layer("Covers").unwrap().visible);
    assert!(
        !dst.layer("Fixtures").unwrap().visible,
        "the document's own layer wins"
    );
}
