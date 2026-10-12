// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Rust pick index has no lazy, capped mode (it holds every placement's
//! AABB span and caches each prototype's triangles once), so the tests
//! comparing the lazy answers with the full ones have nothing to compare; a
//! ray through the instanced field still has to land on the right cube.

use crate::common;

use common::{sized_viewport, v3};
use glam::{DMat4, DVec3};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

#[test]
fn test_a_ray_bakes_what_it_crosses() {
    let mut vp = sized_viewport(1000., 700.);
    let mut m = Mesh::new();
    let p = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
        v3(0., 0., 1.),
        v3(1., 0., 1.),
        v3(1., 1., 1.),
        v3(0., 1., 1.),
    ];
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
    let proto = add_mesh(&mut vp.scene.doc.meshes, m);
    for i in 0..6 {
        for j in 0..4 {
            let mut g = Group::new(proto, Some(&format!("C{i}{j}")));
            let mut x = DMat4::from_translation(v3(3.0 * i as f64, 3.0 * j as f64, 0.));
            if (i + j) % 3 == 0 {
                x *= DMat4::from_scale(v3(-1., 1., 1.)); // some mirrored
            }
            g.xform = Some(x);
            vp.scene.doc.groups.push(g);
        }
    }
    vp.scene.version += 1;
    let g = vp
        .scene
        .doc
        .groups
        .iter()
        .find(|g| g.name == "C21")
        .unwrap();
    let c = g.xform.unwrap().transform_point3(v3(0.5, 0.5, 1.0));
    let d = vp
        .ray_distance(v3(c.x, c.y, 10.0), DVec3::new(0., 0., -1.))
        .unwrap();
    assert!((d - 9.0).abs() < 1e-5); // the top of C21
}
