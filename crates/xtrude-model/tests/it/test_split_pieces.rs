// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Split into Pieces: a group regrouped by the solids that do not touch.

use crate::common;
use common::*;
use std::collections::BTreeSet;
use xtrude_core::math::{DMat4, DVec3};
use xtrude_core::Mesh;
use xtrude_model::commands::groups::SplitIntoPiecesCommand;
use xtrude_model::group::{add_mesh, world_mesh, Group, MeshTable};
use xtrude_model::history::Command;
use xtrude_model::parts::{part_points, part_size};
use xtrude_model::pieces::{connected_parts, solid_parts, split_into_pieces};
use xtrude_model::scene::Scene;

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn corners(x0: f64, z0: f64) -> Vec<DVec3> {
    let mut out = Vec::new();
    for dz in [0.0, 1.0] {
        for dy in [0.0, 1.0] {
            for dx in [0.0, 1.0] {
                out.push(V3(x0 + dx, dy, z0 + dz));
            }
        }
    }
    out
}

fn cube(mesh: &mut Mesh, x0: f64, z0: f64, quads: &[[usize; 4]]) {
    let c = corners(x0, z0);
    for q in quads {
        let ring: Vec<DVec3> = q.iter().map(|&k| c[k]).collect();
        mesh.add_face(&ring, &[]);
    }
}

fn group(t: &mut MeshTable, m: Mesh, name: Option<&str>) -> Group {
    let id = add_mesh(t, m);
    Group::new(id, name)
}

fn points(t: &MeshTable, g: &Group) -> BTreeSet<(i64, i64, i64)> {
    let m = world_mesh(t, g);
    m.verts
        .iter()
        .map(|(_, v)| {
            let p = v.position;
            (
                (p.x * 1e5).round() as i64,
                (p.y * 1e5).round() as i64,
                (p.z * 1e5).round() as i64,
            )
        })
        .collect()
}

#[test]
fn test_two_separate_solids_become_two_pieces() {
    let mut t = MeshTable::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 5.0, 0.0, &QUADS);
    let parts = connected_parts(&mesh);
    assert_eq!(parts.iter().map(|p| p.0.len()).collect::<Vec<_>>(), [6, 6]);
    let g = group(&mut t, mesh, None);
    let pieces = split_into_pieces(&mut t, &g);
    assert_eq!(
        pieces.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Piece 1", "Piece 2"]
    );
    assert!(!pieces.iter().any(|p| p.component));
    assert!(pieces.iter().all(|p| t[&p.mesh].faces.len() == 6));
}

#[test]
fn test_fragments_across_children_join_into_the_piece_they_are() {
    let mut t = MeshTable::new();
    let (mut a, mut b) = (Mesh::new(), Mesh::new());
    cube(&mut a, 0.0, 0.0, &QUADS[..3]);
    cube(&mut b, 0.0, 0.0, &QUADS[3..]);
    let mut halves = Vec::new();
    for m in [a, b] {
        let mut g = group(&mut t, m, None);
        g.xform = Some(DMat4::IDENTITY);
        halves.push(g);
    }
    let mut stool = group(&mut t, Mesh::new(), None);
    stool.adopt(halves);
    let pieces = split_into_pieces(&mut t, &stool);
    assert_eq!(pieces.len(), 1);
    assert_eq!(t[&pieces[0].mesh].faces.len(), 6);
}

#[test]
fn test_a_piece_keeps_the_name_of_the_file_part_it_came_from() {
    let mut t = MeshTable::new();
    let mut named = Vec::new();
    for (name, starts) in [
        ("Carcass", &[0.0, 10.0][..]),
        ("Worktop", &[3.0]),
        ("Part 7", &[6.0]),
    ] {
        let mut m = Mesh::new();
        for &x0 in starts {
            cube(&mut m, x0, 0.0, &QUADS);
        }
        let mut g = group(&mut t, m, Some(name));
        g.xform = Some(DMat4::IDENTITY);
        named.push(g);
    }
    let mut unit = group(&mut t, Mesh::new(), None);
    unit.adopt(named);
    let pieces = split_into_pieces(&mut t, &unit);
    let mut names: Vec<String> = pieces.iter().map(|p| p.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["Carcass", "Carcass 2", "Piece 4", "Worktop"]);
}

/// Only English automatic names ("Part N", "Piece N") give way to the
/// piece's own number; a name in another language, such as "Pieza 7", is
/// the user's and is kept.
#[test]
fn test_only_english_automatic_names_give_way() {
    let mut t = MeshTable::new();
    let mut named = Vec::new();
    for (name, starts) in [("Pieza 7", &[0.0, 10.0][..]), ("Piece 9", &[3.0])] {
        let mut m = Mesh::new();
        for &x0 in starts {
            cube(&mut m, x0, 0.0, &QUADS);
        }
        let mut g = group(&mut t, m, Some(name));
        g.xform = Some(DMat4::IDENTITY);
        named.push(g);
    }
    let mut unit = group(&mut t, Mesh::new(), None);
    unit.adopt(named);
    let pieces = split_into_pieces(&mut t, &unit);
    let mut names: Vec<String> = pieces.iter().map(|p| p.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["Piece 3", "Pieza 7", "Pieza 7 2"]);
}

#[test]
fn test_a_single_piece_has_nothing_to_split() {
    let mut t = MeshTable::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    let g = group(&mut t, mesh, None);
    assert!(split_into_pieces(&mut t, &g).is_empty());
}

#[test]
fn test_splitting_twice_changes_nothing() {
    let mut scene = Scene::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 5.0, 0.0, &QUADS);
    let g = group(&mut scene.doc.meshes, mesh, None);
    let gid = g.id;
    scene.doc.groups.push(g.clone());
    let pieces = split_into_pieces(&mut scene.doc.meshes, &g);
    SplitIntoPiecesCommand { group: gid, pieces }
        .apply(&mut scene)
        .unwrap();
    let g = scene.doc.find_group(gid).unwrap().clone();
    assert!(split_into_pieces(&mut scene.doc.meshes, &g).is_empty());
}

#[test]
fn test_touching_boards_are_separate_pieces() {
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 0.0, 1.0, &QUADS);
    cube(&mut mesh, 1.0, 0.0, &QUADS);
    assert_eq!(connected_parts(&mesh).len(), 1);
    assert_eq!(
        solid_parts(&mesh)
            .iter()
            .map(|p| p.0.len())
            .collect::<Vec<_>>(),
        [6, 6, 6]
    );
}

#[test]
fn test_a_piece_does_not_take_its_neighbours_edges() {
    let mut t = MeshTable::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 0.0, 1.0, &QUADS);
    let g = group(&mut t, mesh, None);
    let pieces = split_into_pieces(&mut t, &g);
    assert_eq!(
        pieces
            .iter()
            .map(|p| t[&p.mesh].edges.len())
            .collect::<Vec<_>>(),
        [12, 12]
    );
    for p in &pieces {
        let s = part_size(&part_points(&t, p));
        assert!((s.0 - 1.0).abs() < 1e-9 && (s.1 - 1.0).abs() < 1e-9 && (s.2 - 1.0).abs() < 1e-9);
    }
}

#[test]
fn test_fragments_join_the_solid_they_lie_on() {
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    let sheet = [
        V3(0.2, 0., 0.2),
        V3(0.8, 0., 0.2),
        V3(0.8, 0., 0.8),
        V3(0.2, 0., 0.8),
    ];
    mesh.add_face(&sheet, &[]);
    mesh.add_face(&[V3(0., 0., 0.), V3(1., 0., 0.), V3(1., 1., 0.)], &[]);
    let mut sizes: Vec<usize> = solid_parts(&mesh).iter().map(|p| p.0.len()).collect();
    sizes.sort();
    assert_eq!(sizes, [1, 7]);
}

#[test]
fn test_a_zero_thickness_double_sided_sheet_is_not_a_solid() {
    let mut mesh = Mesh::new();
    let pts = [
        V3(0., 0., 0.),
        V3(1., 0., 0.),
        V3(1., 1., 0.),
        V3(0., 1., 0.),
    ];
    mesh.add_face(&pts, &[]);
    let mut rev = pts.to_vec();
    rev.reverse();
    mesh.add_face(&rev, &[]);
    assert!(!xtrude_model::pieces::is_solid(&mesh, &mesh.face_ids()));
}

#[test]
fn test_the_split_keeps_the_geometry_where_it_was_and_undoes() {
    let mut scene = Scene::new();
    let mut history = xtrude_model::history::History::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 5.0, 0.0, &QUADS);
    let f0 = mesh.face_ids()[0];
    mesh.face_mut(f0).attrs.paint.color = Some([0.8, 0.1, 0.1]);
    let mut g = group(&mut scene.doc.meshes, mesh, Some("stool"));
    let original_mesh = g.mesh;
    let place =
        DMat4::from_translation(V3(3., 4., 0.)) * DMat4::from_rotation_z(30f64.to_radians());
    g.xform = Some(place);
    let gid = g.id;
    scene.doc.groups.push(g.clone());
    let before = points(&scene.doc.meshes, &g);

    let pieces = split_into_pieces(&mut scene.doc.meshes, &g);
    assert!(history.execute(
        &mut scene,
        &mut SplitIntoPiecesCommand { group: gid, pieces }
    ));
    let g = scene.doc.find_group(gid).unwrap().clone();
    assert_eq!(scene.doc.groups.len(), 1);
    assert_eq!(g.name, "stool");
    assert_eq!(g.xform, Some(place));
    assert!(scene.doc.meshes[&g.mesh].faces.is_empty());
    assert_eq!(g.children.len(), 2);
    assert_eq!(points(&scene.doc.meshes, &g), before);
    let painted: usize = g
        .children
        .iter()
        .map(|k| {
            let m = &scene.doc.meshes[&k.mesh];
            m.face_ids()
                .iter()
                .filter(|&&f| m.face(f).attrs.paint.color.is_some())
                .count()
        })
        .sum();
    assert_eq!(painted, 1);

    history.undo(&mut scene);
    let g = scene.doc.find_group(gid).unwrap();
    assert_eq!(g.mesh, original_mesh);
    assert!(g.children.is_empty());
    assert_eq!(g.xform, Some(place));
}

#[test]
fn test_edge_flags_travel_with_the_piece() {
    let mut t = MeshTable::new();
    let mut mesh = Mesh::new();
    cube(&mut mesh, 0.0, 0.0, &QUADS);
    cube(&mut mesh, 5.0, 0.0, &QUADS);
    for e in mesh.edge_ids().into_iter().take(2) {
        mesh.edge_mut(e).hidden = true;
        mesh.edge_mut(e).soft = true;
    }
    let g = group(&mut t, mesh, None);
    let pieces = split_into_pieces(&mut t, &g);
    let count = |pred: &dyn Fn(&xtrude_core::mesh::Edge) -> bool| -> usize {
        pieces
            .iter()
            .map(|p| t[&p.mesh].edges.iter().filter(|(_, e)| pred(e)).count())
            .sum()
    };
    assert_eq!(count(&|e| e.hidden), 2);
    assert_eq!(count(&|e| e.soft), 2);
}
