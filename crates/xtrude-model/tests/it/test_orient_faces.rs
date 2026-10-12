// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Orient Faces: the faces connected to the chosen one wind like
//! it.

use std::collections::{HashMap, HashSet};
use xtrude_core::math::VecExt;
use xtrude_core::orient_faces::{connected_count, faces_to_flip};
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::commands::FlipFacesCommand;
use xtrude_model::history::Command;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn normal(m: &Mesh, f: FaceId) -> DVec3 {
    xtrude_core::math::newell(&m.face_positions(f)).unit_or_zero()
}

fn agree(m: &Mesh) -> bool {
    for e in m.edge_ids() {
        let ed = m.edge(e);
        if ed.faces.len() != 2 {
            continue;
        }
        let mut dirs = Vec::new();
        for &f in &ed.faces {
            let lp = &m.face(f).outer;
            for i in 0..lp.len() {
                let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
                if (a == ed.v0 && b == ed.v1) || (a == ed.v1 && b == ed.v0) {
                    dirs.push((a, b));
                }
            }
        }
        if dirs[0] == dirs[1] {
            return false;
        }
    }
    true
}

fn cube(scrambled: bool) -> (Mesh, HashMap<&'static str, FaceId>) {
    let mut m = Mesh::new();
    let mut c = Vec::new();
    for z in [0., 1.] {
        for y in [0., 1.] {
            for x in [0., 1.] {
                c.push(v(x, y, z));
            }
        }
    }
    let quads: [(&str, [usize; 4]); 6] = [
        ("bottom", [0, 2, 3, 1]),
        ("top", [4, 5, 7, 6]),
        ("front", [0, 1, 5, 4]),
        ("back", [2, 6, 7, 3]),
        ("left", [0, 4, 6, 2]),
        ("right", [1, 3, 7, 5]),
    ];
    let mut faces = HashMap::new();
    for (name, mut q) in quads {
        if scrambled && ["top", "left", "back"].contains(&name) {
            q.reverse();
        }
        let pts: Vec<DVec3> = q.iter().map(|&i| c[i]).collect();
        faces.insert(name, m.add_face(&pts, &[]));
    }
    (m, faces)
}

/// `FlipFacesCommand(faces).do(_Scene(mesh))`
fn flip(m: &mut Mesh, faces: &[FaceId]) {
    let mut s = Scene::new();
    s.doc
        .meshes
        .insert(MeshId::LOOSE, std::sync::Arc::new(m.clone()));
    let mut cmd = FlipFacesCommand {
        faces: faces.iter().map(|&f| (MeshId::LOOSE, f)).collect(),
    };
    cmd.apply(&mut s).unwrap();
    *m = s.doc.mesh(MeshId::LOOSE).clone();
}

#[test]
fn test_a_scrambled_cube_winds_like_the_chosen_face() {
    let (mut m, faces) = cube(true);
    let seed = faces["bottom"];
    let before = normal(&m, seed);
    let fl = faces_to_flip(&m, seed);
    let got: HashSet<FaceId> = fl.iter().copied().collect();
    let want: HashSet<FaceId> = ["top", "left", "back"].iter().map(|k| faces[k]).collect();
    assert_eq!(got, want);
    flip(&mut m, &fl);
    assert!(agree(&m));
    assert_eq!(normal(&m, seed), before);
    assert!(normal(&m, faces["top"]).z > 0.99);
    assert!(normal(&m, faces["left"]).x < -0.99);
}

#[test]
fn test_choosing_an_inward_face_turns_the_whole_cube_inward() {
    let (mut m, faces) = cube(false);
    flip(&mut m, &[faces["front"]]);
    let fl = faces_to_flip(&m, faces["front"]);
    flip(&mut m, &fl);
    assert!(agree(&m));
    assert!(normal(&m, faces["top"]).z < -0.99);
}

#[test]
fn test_an_already_consistent_surface_needs_nothing() {
    let (m, faces) = cube(false);
    assert!(faces_to_flip(&m, faces["top"]).is_empty());
    assert_eq!(connected_count(&m, faces["top"]), 6);
}

#[test]
fn test_an_open_surface_follows_the_chosen_face() {
    let mut m = Mesh::new();
    let a = m.add_face(
        &[v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)],
        &[],
    );
    let b = m.add_face(
        &[v(1., 0., 0.), v(1., 1., 0.), v(2., 1., 0.), v(2., 0., 0.)],
        &[],
    );
    assert_eq!(faces_to_flip(&m, a), vec![b]);
    assert_eq!(faces_to_flip(&m, b), vec![a]);
}

#[test]
fn test_it_stops_where_three_faces_meet_an_edge() {
    let mut m = Mesh::new();
    let a = m.add_face(
        &[v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)],
        &[],
    );
    m.add_face(
        &[v(1., 0., 0.), v(1., 1., 0.), v(2., 1., 0.), v(2., 0., 0.)],
        &[],
    );
    m.add_face(
        &[v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.), v(1., 0., 1.)],
        &[],
    );
    assert!(faces_to_flip(&m, a).is_empty());
    assert_eq!(connected_count(&m, a), 1);
}
