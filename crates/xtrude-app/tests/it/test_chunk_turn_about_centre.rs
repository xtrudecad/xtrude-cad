// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A group turned about its own centre must be redrawn,
//! although a turn about the centroid keeps the sum of its coordinates. The
//! frame cache follows the scene version; what is checked is that the drawn
//! triangles are where the vertices are.

use crate::common;

use glam::DVec3;
use xtrude_core::Mesh;
use xtrude_model::commands::RotateGroupCommand;
use xtrude_model::group::{add_mesh, Group};

fn turn(deg: f64) {
    let mut vp = common::sized_viewport(300., 300.);
    let p = [
        DVec3::new(0., 0., 0.),
        DVec3::new(2., 0., 0.),
        DVec3::new(2., 2., 0.),
        DVec3::new(0., 2., 0.),
        DVec3::new(0., 0., 2.),
        DVec3::new(2., 0., 2.),
        DVec3::new(2., 2., 2.),
        DVec3::new(0., 2., 2.),
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
        m.add_face(&idx.map(|i| p[i]), &[]);
    }
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, None);
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.frame_data([300, 300], false);
    vp.run(RotateGroupCommand {
        group: gid,
        center: DVec3::new(1., 1., 1.),
        axis: DVec3::Z,
        degrees: deg,
    });
    let f = vp.frame_data([300, 300], false);
    let mesh = vp.scene.doc.mesh(mid);
    let live: Vec<DVec3> = mesh.vertex_ids().into_iter().map(|v| mesh.pos(v)).collect();
    for fv in f.scene_faces.data.iter() {
        let w = f.origin + DVec3::new(fv.pos[0] as f64, fv.pos[1] as f64, fv.pos[2] as f64);
        assert!(
            live.iter().any(|q| (*q - w).length() < 1e-4),
            "{deg}°: a triangle corner at {w} is not a vertex"
        );
    }
}

#[test]
fn test_turning_a_group_about_its_centre_rebuilds_its_chunk() {
    for deg in [30.0, 90.0] {
        turn(deg);
    }
}
