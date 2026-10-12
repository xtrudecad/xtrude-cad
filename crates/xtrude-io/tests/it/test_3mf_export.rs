// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3MF export: the package, its objects, items and colours.

use crate::common;
use common::*;
use std::io::Read;
use std::path::Path;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_io::threemf::save_3mf;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::scene::Scene;

/// A box from `lo` to `hi` (metres), every face wound outward.
fn cuboid(m: &mut Mesh, lo: DVec3, hi: DVec3) -> Vec<xtrude_core::FaceId> {
    let p = |x: bool, y: bool, z: bool| {
        DVec3::new(
            if x { hi.x } else { lo.x },
            if y { hi.y } else { lo.y },
            if z { hi.z } else { lo.z },
        )
    };
    let (f, t) = (false, true);
    [
        [p(f, f, f), p(f, t, f), p(t, t, f), p(t, f, f)],
        [p(f, f, t), p(t, f, t), p(t, t, t), p(f, t, t)],
        [p(f, f, f), p(t, f, f), p(t, f, t), p(f, f, t)],
        [p(f, t, f), p(f, t, t), p(t, t, t), p(t, t, f)],
        [p(f, f, f), p(f, f, t), p(f, t, t), p(f, t, f)],
        [p(t, f, f), p(t, t, f), p(t, t, t), p(t, f, t)],
    ]
    .iter()
    .map(|ring| m.add_face(ring, &[]))
    .collect()
}

fn cube_mesh(at: DVec3) -> Mesh {
    let mut m = Mesh::new();
    cuboid(&mut m, at, at + DVec3::splat(0.01));
    m
}

/// The parts of a written package, by name.
struct Package {
    names: Vec<String>,
    content_types: String,
    rels: String,
    model: String,
}

fn read_package(path: &Path) -> Package {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let names = z.file_names().map(str::to_string).collect();
    let mut part = |name: &str| {
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    Package {
        names,
        content_types: part("[Content_Types].xml"),
        rels: part("_rels/.rels"),
        model: part("3D/3dmodel.model"),
    }
}

fn export(s: &Scene) -> Package {
    let tmp = TmpDir::new();
    let path = tmp.join("out.3mf");
    save_3mf(s, &path).unwrap();
    read_package(&path)
}

struct Obj {
    id: String,
    name: String,
    pid: Option<String>,
    pindex: Option<String>,
    vertices: Vec<DVec3>,
    triangles: Vec<([usize; 3], Option<String>)>,
}

fn children<'a, 'i>(
    n: roxmltree::Node<'a, 'i>,
    tag: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.descendants().filter(move |c| c.has_tag_name(tag))
}

fn f(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).unwrap().parse().unwrap()
}

fn objects(doc: &roxmltree::Document) -> Vec<Obj> {
    children(doc.root(), "object")
        .map(|o| Obj {
            id: o.attribute("id").unwrap().into(),
            name: o.attribute("name").unwrap_or("").into(),
            pid: o.attribute("pid").map(Into::into),
            pindex: o.attribute("pindex").map(Into::into),
            vertices: children(o, "vertex")
                .map(|v| DVec3::new(f(v, "x"), f(v, "y"), f(v, "z")))
                .collect(),
            triangles: children(o, "triangle")
                .map(|t| {
                    let i = |a| t.attribute(a).unwrap().parse().unwrap();
                    (
                        [i("v1"), i("v2"), i("v3")],
                        t.attribute("p1").map(Into::into),
                    )
                })
                .collect(),
        })
        .collect()
}

/// `(objectid, transform)` of every build item.
fn items(doc: &roxmltree::Document) -> Vec<(String, Option<String>)> {
    children(doc.root(), "item")
        .map(|i| {
            (
                i.attribute("objectid").unwrap().into(),
                i.attribute("transform").map(Into::into),
            )
        })
        .collect()
}

fn bases(doc: &roxmltree::Document) -> Vec<String> {
    children(doc.root(), "base")
        .map(|b| b.attribute("displaycolor").unwrap().into())
        .collect()
}

#[test]
fn test_the_package_has_the_three_parts_with_their_content_types() {
    let mut s = Scene::new();
    *s.mesh_mut() = cube_mesh(DVec3::ZERO);
    let p = export(&s);
    for name in ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"] {
        assert!(p.names.iter().any(|n| n == name), "{name} missing");
    }
    let types = roxmltree::Document::parse(&p.content_types).unwrap();
    let ct = |ext: &str| {
        children(types.root(), "Default")
            .find(|d| d.attribute("Extension") == Some(ext))
            .and_then(|d| d.attribute("ContentType"))
            .map(str::to_string)
    };
    assert_eq!(
        ct("model").as_deref(),
        Some("application/vnd.ms-package.3dmanufacturing-3dmodel+xml")
    );
    assert_eq!(
        ct("rels").as_deref(),
        Some("application/vnd.openxmlformats-package.relationships+xml")
    );
    let rels = roxmltree::Document::parse(&p.rels).unwrap();
    let rel = children(rels.root(), "Relationship").next().unwrap();
    assert_eq!(rel.attribute("Target"), Some("/3D/3dmodel.model"));
    assert_eq!(
        rel.attribute("Type"),
        Some("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel")
    );
    let model = roxmltree::Document::parse(&p.model).unwrap();
    let root = model.root_element();
    assert_eq!(root.tag_name().name(), "model");
    assert_eq!(
        root.tag_name().namespace(),
        Some("http://schemas.microsoft.com/3dmanufacturing/core/2015/02")
    );
    assert_eq!(root.attribute("unit"), Some("millimeter"));
}

#[test]
fn test_a_10_mm_cube_is_one_object_of_8_vertices_and_12_outward_triangles() {
    let mut s = Scene::new();
    *s.mesh_mut() = cube_mesh(DVec3::ZERO);
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    let objs = objects(&doc);
    assert_eq!(objs.len(), 1);
    let o = &objs[0];
    assert_eq!(o.vertices.len(), 8);
    assert_eq!(o.triangles.len(), 12);
    let (lo, hi) = o.vertices.iter().fold(
        (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
        |(lo, hi), &v| (lo.min(v), hi.max(v)),
    );
    assert!(lo.abs_diff_eq(DVec3::ZERO, 1e-9), "{lo}");
    assert!(hi.abs_diff_eq(DVec3::splat(10.0), 1e-9), "{hi}");
    let centre = DVec3::splat(5.0);
    for ([a, b, c], _) in &o.triangles {
        let (a, b, c) = (o.vertices[*a], o.vertices[*b], o.vertices[*c]);
        let n = (b - a).cross(c - a);
        assert!(n.dot((a + b + c) / 3.0 - centre) > 0.0, "inward triangle");
    }
    assert_eq!(items(&doc), [(o.id.clone(), None)]);
    // One colour: the object carries it, the triangles do not.
    assert_eq!(bases(&doc).len(), 1);
    assert_eq!(o.pid.as_deref(), Some("1"));
    assert_eq!(o.pindex.as_deref(), Some("0"));
    assert!(o.triangles.iter().all(|t| t.1.is_none()));
}

#[test]
fn test_two_groups_are_two_objects_and_two_items() {
    let mut s = Scene::new();
    for (x, name) in [(0.0, "Lid"), (0.02, "Base")] {
        let mesh = add_mesh(&mut s.doc.meshes, cube_mesh(DVec3::new(x, 0., 0.)));
        let mut g = Group::new(mesh, Some(name));
        g.component = false;
        s.doc.groups.push(g);
    }
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    let objs = objects(&doc);
    assert_eq!(
        objs.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["Lid", "Base"]
    );
    assert!(objs.iter().all(|o| o.vertices.len() == 8));
    let max_x = objs[1]
        .vertices
        .iter()
        .map(|v| v.x)
        .fold(f64::MIN, f64::max);
    assert!((max_x - 30.0).abs() < 1e-9);
    assert_eq!(
        items(&doc),
        [(objs[0].id.clone(), None), (objs[1].id.clone(), None)]
    );
}

#[test]
fn test_two_instances_of_a_component_share_one_object_placed_twice() {
    let mut s = Scene::new();
    let mesh = add_mesh(&mut s.doc.meshes, cube_mesh(DVec3::ZERO));
    for x in [0.0, 0.05] {
        let mut g = Group::new(mesh, Some("Peg"));
        g.xform = Some(DMat4::from_translation(DVec3::new(x, 0.002, 0.)));
        s.doc.groups.push(g);
    }
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    let objs = objects(&doc);
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0].name, "Peg");
    assert_eq!(objs[0].vertices.len(), 8);
    let id = objs[0].id.clone();
    assert_eq!(
        items(&doc),
        [
            (id.clone(), Some("1 0 0 0 1 0 0 0 1 0 2 0".to_string())),
            (id, Some("1 0 0 0 1 0 0 0 1 50 2 0".to_string())),
        ]
    );
}

#[test]
fn test_a_mirrored_instance_is_written_in_place_and_stays_outward() {
    let mut s = Scene::new();
    let mesh = add_mesh(&mut s.doc.meshes, cube_mesh(DVec3::ZERO));
    let mut g = Group::new(mesh, Some("Left"));
    g.xform = Some(DMat4::from_scale(DVec3::new(-1., 1., 1.)));
    s.doc.groups.push(g);
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    let objs = objects(&doc);
    assert_eq!(items(&doc), [(objs[0].id.clone(), None)]);
    let o = &objs[0];
    let centre = DVec3::new(-5., 5., 5.);
    for ([a, b, c], _) in &o.triangles {
        let (a, b, c) = (o.vertices[*a], o.vertices[*b], o.vertices[*c]);
        assert!(a.x <= 1e-9 && b.x <= 1e-9 && c.x <= 1e-9);
        let n = (b - a).cross(c - a);
        assert!(n.dot((a + b + c) / 3.0 - centre) > 0.0, "inward triangle");
    }
}

#[test]
fn test_two_colours_on_one_solid_are_two_base_materials_per_triangle() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    let faces = cuboid(&mut m, DVec3::ZERO, DVec3::splat(0.01));
    for &f in &faces[..2] {
        m.face_mut(f).attrs.paint.color = Some([1., 0., 0.]);
    }
    for &f in &faces[2..] {
        m.face_mut(f).attrs.paint.color = Some([0., 0., 1.]);
    }
    *s.mesh_mut() = m;
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    assert_eq!(bases(&doc), ["#FF0000", "#0000FF"]);
    let objs = objects(&doc);
    assert_eq!(objs.len(), 1);
    let o = &objs[0];
    assert_eq!(o.pid.as_deref(), Some("1"));
    let p1: Vec<_> = o.triangles.iter().map(|t| t.1.as_deref()).collect();
    assert_eq!(p1.iter().filter(|p| **p == Some("0")).count(), 4);
    assert_eq!(p1.iter().filter(|p| **p == Some("1")).count(), 8);
    assert!(children(doc.root(), "triangle").all(|t| t.attribute("pid") == Some("1")));
}

#[test]
fn test_a_group_paint_colours_its_unpainted_faces() {
    let mut s = Scene::new();
    let mesh = add_mesh(&mut s.doc.meshes, cube_mesh(DVec3::ZERO));
    let mut g = Group::new(mesh, Some("Knob"));
    g.component = false;
    g.material = Some(xtrude_core::attrs::Paint {
        color: Some([0., 1., 0.]),
        ..Default::default()
    });
    s.doc.groups.push(g);
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    assert_eq!(bases(&doc), ["#00FF00"]);
}

#[test]
fn test_names_are_escaped() {
    let mut s = Scene::new();
    let mesh = add_mesh(&mut s.doc.meshes, cube_mesh(DVec3::ZERO));
    let mut g = Group::new(mesh, Some("A & <B> \"C\""));
    g.component = false;
    s.doc.groups.push(g);
    let p = export(&s);
    let doc = roxmltree::Document::parse(&p.model).unwrap();
    assert_eq!(objects(&doc)[0].name, "A & <B> \"C\"");
}
