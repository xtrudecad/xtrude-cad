// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The properties panel reads a face's area and a solid's volume in the
//! document's units, and names a face's colour as Model Info does.

use crate::common;

use common::{v3, window};
use glam::DVec3;
use xtrude_app::tray::properties::Properties;
use xtrude_app::window::MainWindow;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::units::Units;

/// A loose 10 × 10 mm face.
fn square_face(w: &mut MainWindow) -> FaceId {
    let s = 0.010;
    let f = w.viewport.scene.mesh_mut().add_face(
        &[v3(0., 0., 0.), v3(s, 0., 0.), v3(s, s, 0.), v3(0., s, 0.)],
        &[],
    );
    w.viewport.scene.version += 1;
    f
}

/// A group holding a closed 20 mm cube.
fn cube_group(w: &mut MainWindow) -> GroupId {
    let (lo, hi) = (DVec3::new(0.1, 0., 0.), DVec3::new(0.12, 0.02, 0.02));
    let p = [
        v3(lo.x, lo.y, lo.z),
        v3(hi.x, lo.y, lo.z),
        v3(hi.x, hi.y, lo.z),
        v3(lo.x, hi.y, lo.z),
        v3(lo.x, lo.y, hi.z),
        v3(hi.x, lo.y, hi.z),
        v3(hi.x, hi.y, hi.z),
        v3(lo.x, hi.y, hi.z),
    ];
    let mut m = Mesh::new();
    for idx in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let pts: Vec<DVec3> = idx.iter().map(|&i| p[i]).collect();
        m.add_face(&pts, &[]);
    }
    let g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some("Cube"));
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

/// The panel's text for the face and for the cube, in a document in `units`.
fn readouts(units: Units) -> (String, String) {
    let mut w = window();
    w.viewport.scene.apply_units(&units);
    let f = square_face(&mut w);
    let g = cube_group(&mut w);
    let mut p = Properties::default();
    let face = p.describe(&w, &[EntityRef::Face(MeshId::LOOSE, f)]);
    let cube = p.describe(&w, &[EntityRef::Group(g)]);
    (face, cube)
}

#[test]
fn test_millimetre_document() {
    let (face, cube) = readouts(Units::new("mm", 2));
    assert!(face.contains("Area: 100.00 mm²"), "{face}");
    assert!(cube.contains("Solid Group"), "{cube}");
    assert!(cube.contains("Volume: 8.00 cm³"), "{cube}");
}

#[test]
fn test_metre_document() {
    let (face, cube) = readouts(Units::new("m", 6));
    assert!(face.contains("Area: 0.000100 m²"), "{face}");
    assert!(cube.contains("Volume: 0.000008 m³"), "{cube}");
}

#[test]
fn test_inch_document() {
    let (face, cube) = readouts(Units::new("in", 3));
    assert!(face.contains("Area: 0.155 in²"), "{face}");
    assert!(cube.contains("Volume: 0.488 in³"), "{cube}");
}

#[test]
fn test_the_colour_line_names_the_colour() {
    let mut w = window();
    let f = square_face(&mut w);
    let mut p = Properties::default();
    let sel = [EntityRef::Face(MeshId::LOOSE, f)];
    assert!(p.describe(&w, &sel).contains("Color: —"));
    w.viewport.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.8, 0.2, 0.2]);
    let text = p.describe(&w, &sel);
    assert!(text.contains("Color: rgb(204,51,51)"), "{text}");
    w.viewport.scene.mesh_mut().face_mut(f).attrs.paint.mat = Some("Signal red".into());
    let text = p.describe(&w, &sel);
    assert!(text.contains("Color: Signal red"), "{text}");
}
