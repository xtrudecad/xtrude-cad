// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The automatic orientation pass must not move paint to the other side. A
//! face painted on BOTH sides keeps each paint where it was; paint on one side
//! follows the correction; the user's Reverse Faces keeps its classic meaning.

use crate::common;

use common::{stub, v3};
use std::collections::HashMap;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_core::attrs::{Back, Paint};
use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::FlipFacesCommand;
use xtrude_model::ids::MeshId;

fn brick() -> Paint {
    Paint {
        color: Some([0.7, 0.3, 0.2]),
        mat: Some("Brick red".into()),
        opacity: None,
    }
}

fn plaster() -> Paint {
    Paint {
        color: Some([0.95, 0.95, 0.9]),
        mat: Some("Plaster".into()),
        opacity: None,
    }
}

/// A unit cube, every face outward except "south", wound the wrong way.
fn cube(mesh: &mut Mesh) -> HashMap<&'static str, FaceId> {
    let mut p = Vec::new();
    for z in [0., 1.] {
        for y in [0., 1.] {
            for x in [0., 1.] {
                p.push(v3(x, y, z));
            }
        }
    }
    let quads: [(&str, [usize; 4]); 6] = [
        ("bottom", [0, 2, 3, 1]),
        ("top", [4, 5, 7, 6]),
        ("south", [0, 1, 5, 4]),
        ("north", [2, 6, 7, 3]),
        ("west", [0, 4, 6, 2]),
        ("east", [1, 3, 7, 5]),
    ];
    let mut faces = HashMap::new();
    for (name, idx) in quads {
        let mut lp: Vec<_> = idx.iter().map(|&i| p[i]).collect();
        if name == "south" {
            lp.reverse();
        }
        faces.insert(name, mesh.add_face(&lp, &[]));
    }
    faces
}

#[test]
fn test_orient_outward_swaps_the_paints_with_the_winding() {
    let mut mesh = Mesh::new();
    let south = cube(&mut mesh)["south"];
    assert!(mesh.face_normal(south).y > 0.99); // wound inward
                                               // Clicking from OUTSIDE hits this inverted face's BACK → brick there.
    mesh.face_mut(south).attrs.paint = plaster();
    mesh.face_mut(south).attrs.back = Some(Back::Own(brick()));
    let flipped = orient_outward(&mut mesh, DEFAULT_SEED, None);
    assert!(flipped.contains(&south) && mesh.face_normal(south).y < -0.99);
    assert_eq!(mesh.face(south).attrs.paint, brick());
    assert_eq!(mesh.face(south).attrs.back, Some(Back::Own(plaster())));
}

#[test]
fn test_paint_on_the_back_alone_moves_to_the_front_with_the_turn() {
    let mut mesh = Mesh::new();
    let south = cube(&mut mesh)["south"];
    mesh.face_mut(south).attrs.back = Some(Back::Own(brick()));
    orient_outward(&mut mesh, DEFAULT_SEED, None);
    assert_eq!(mesh.face(south).attrs.paint, brick());
    assert_eq!(mesh.face(south).attrs.back, None);
}

#[test]
fn test_paint_on_the_front_alone_follows_the_correction() {
    let mut mesh = Mesh::new();
    let south = cube(&mut mesh)["south"];
    mesh.face_mut(south).attrs.paint = plaster();
    orient_outward(&mut mesh, DEFAULT_SEED, None);
    assert!(mesh.face_normal(south).y < -0.99);
    assert_eq!(mesh.face(south).attrs.paint, plaster());
    assert_eq!(mesh.face(south).attrs.back, None);
}

#[test]
fn test_a_mirrored_two_sided_face_is_left_alone() {
    let mut mesh = Mesh::new();
    let south = cube(&mut mesh)["south"];
    mesh.face_mut(south).attrs.paint = brick();
    mesh.face_mut(south).attrs.back = Some(Back::Mirror);
    orient_outward(&mut mesh, DEFAULT_SEED, None);
    assert!(mesh.face_normal(south).y < -0.99);
    assert_eq!(mesh.face(south).attrs.paint, brick());
    assert_eq!(mesh.face(south).attrs.back, Some(Back::Mirror));
}

#[test]
fn test_turn_face_over_is_an_involution() {
    let mut mesh = Mesh::new();
    let south = cube(&mut mesh)["south"];
    mesh.face_mut(south).attrs.paint = plaster();
    mesh.face_mut(south).attrs.back = Some(Back::Own(brick()));
    let before = (
        mesh.face(south).attrs.clone(),
        mesh.face(south).outer.clone(),
    );
    mesh.turn_face_over(south);
    mesh.turn_face_over(south);
    assert_eq!(
        (
            mesh.face(south).attrs.clone(),
            mesh.face(south).outer.clone()
        ),
        before
    );
}

#[test]
fn test_reverse_faces_keeps_its_classic_meaning() {
    let mut vp = stub();
    let south = cube(vp.scene.mesh_mut())["south"];
    vp.scene.mesh_mut().face_mut(south).attrs.paint = plaster();
    vp.scene.mesh_mut().face_mut(south).attrs.back = Some(Back::Own(brick()));
    vp.execute(&mut FlipFacesCommand {
        faces: vec![(MeshId::LOOSE, south)],
    });
    let m = vp.scene.mesh();
    assert!(m.face_normal(south).y < -0.99);
    assert_eq!(m.face(south).attrs.paint, plaster());
    assert_eq!(m.face(south).attrs.back, Some(Back::Own(brick())));
}

#[test]
fn test_pushing_an_inverted_wall_from_inside_keeps_the_brick_outside() {
    let mut vp = stub();
    let south = cube(vp.scene.mesh_mut())["south"];
    vp.scene.mesh_mut().face_mut(south).attrs.paint = plaster();
    vp.scene.mesh_mut().face_mut(south).attrs.back = Some(Back::Own(brick()));
    let mut pp = PushPullTool::new();
    pp.begin(&mut vp, MeshId::LOOSE, south);
    pp.pp.extrusion = 0.3; // a bump out toward the street
    pp.commit(&mut vp);
    let m = vp.scene.mesh();
    let street: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.y + 0.3).abs() < 1e-6))
        .collect();
    assert_eq!(street.len(), 1);
    let cap = street[0];
    assert!(m.face_normal(cap).y < -0.99);
    assert_eq!(m.face(cap).attrs.paint, brick());
    assert_eq!(m.face(cap).attrs.back, Some(Back::Own(plaster())));
}
