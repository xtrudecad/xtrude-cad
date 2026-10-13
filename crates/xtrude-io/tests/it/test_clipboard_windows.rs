// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Copy/Paste between windows: the wire format (the system-clipboard tests
//! live with the app).

use crate::common;
use common::*;
use xtrude_core::attrs::{FaceAttrs, Paint};
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::Mesh;
use xtrude_io::clip::{decode, encode, ClipData};
use xtrude_model::group::{add_mesh, copy_group, Group, MeshTable};

fn box_group(t: &mut MeshTable, x: f64) -> Group {
    let mut m = Mesh::new();
    quad(&mut m, x, x + 1., 0., 1., 0.);
    let mut g = Group::new(add_mesh(t, m), None);
    g.name = format!("box {x}");
    g
}

fn clip_data() -> ClipData {
    let mut t = MeshTable::new();
    let square = vec![
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 2., 0.),
        v3(0., 2., 0.),
    ];
    let hole = vec![
        v3(0.5, 0.5, 0.),
        v3(1., 0.5, 0.),
        v3(1., 1., 0.),
        v3(0.5, 1., 0.),
    ];
    let classic = box_group(&mut t, 5.0);
    let mut src = box_group(&mut t, 9.0);
    let mut placed = copy_group(&mut t, &mut src, None, false);
    placed.xform = Some(DMat4::from_translation(v3(0., 0., 3.)));
    ClipData {
        faces: vec![(
            square,
            vec![hole],
            FaceAttrs {
                paint: Paint {
                    color: Some([0.8, 0.2, 0.1]),
                    ..Default::default()
                },
                ..Default::default()
            },
        )],
        edges: vec![(v3(0., 0., 0.), v3(0., 0., 4.), true, None)],
        groups: vec![classic, placed],
        meshes: t,
        reference: v3(0., 0., 0.),
    }
}

#[test]
fn test_the_clipboard_survives_the_trip() {
    let out = decode(&encode(&clip_data()).unwrap()).unwrap();
    assert_eq!(out.faces.len(), 1);
    let (lp, holes, attrs) = &out.faces[0];
    assert!(lp.len() == 4 && holes.len() == 1);
    let c = attrs.paint.color.unwrap();
    assert!((c[0] - 0.8).abs() < 1e-9 && (c[1] - 0.2).abs() < 1e-9 && (c[2] - 0.1).abs() < 1e-9);
    assert_eq!(
        out.edges,
        vec![(v3(0., 0., 0.), v3(0., 0., 4.), true, None)]
    );
    assert_eq!(out.reference, v3(0., 0., 0.));
    assert_eq!(out.groups.len(), 2);
    assert!(out.groups.iter().all(|g| g.xform.is_some()));
    assert!((out.groups[1].xform.unwrap().project_point(v3(0., 0., 0.)).z - 3.0).abs() < 1e-9);
    assert!(out.groups.iter().all(|g| out.meshes.contains_key(&g.mesh)));
}

#[test]
fn test_garbage_is_not_a_clipboard() {
    assert!(decode(b"not a zip").is_none());
}

#[test]
fn test_the_scene_travels_as_a_document() {
    use std::io::Read;
    let bytes = encode(&clip_data()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(z.by_name("scene.igz").is_err());
    let mut doc = Vec::new();
    z.by_name("scene.xtr")
        .unwrap()
        .read_to_end(&mut doc)
        .unwrap();
    let mut s = xtrude_model::scene::Scene::new();
    xtrude_io::document::load_bytes(&mut s, &doc, "scene.xtr", None).unwrap();
    assert_eq!(s.mesh().faces.len(), 1);
}
