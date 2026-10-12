// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What you draw inside an open group or component is drawn at once: the new
//! edges are among the scene lines of the frame.

use crate::common;

use common::{v3, viewport};
use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, oriented_box_corners, Group};
use xtrude_render::lines::box_dash_vertices;

fn box_group(vp: &mut Viewport) -> Group {
    let mut m = Mesh::new();
    let c: Vec<DVec3> = [0., 1.]
        .iter()
        .flat_map(|&z| {
            [0., 1.]
                .iter()
                .flat_map(move |&y| [0., 1.].map(move |x| v3(x, y, z)))
        })
        .collect();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        let pts: Vec<DVec3> = q.iter().map(|&i| c[i]).collect();
        m.add_face(&pts, &[]);
    }
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    Group::new(mid, Some("housing"))
}

/// The edges the next frame draws.
fn drawn_edges(vp: &mut Viewport) -> usize {
    vp.frame_data([200, 200], false).scene_lines.data.len() / 2
}

#[test]
fn test_an_edge_drawn_inside_an_open_group_is_drawn_at_once() {
    for component in [false, true] {
        let mut vp = viewport();
        let mut g = box_group(&mut vp);
        if component {
            g.xform = Some(DMat4::IDENTITY); // an instance: edited on a world copy
        }
        let gid = g.id;
        vp.scene.doc.groups.push(g);
        vp.scene.version += 1;
        vp.begin_group_edit(gid);
        let before = drawn_edges(&mut vp);
        vp.scene
            .mesh_mut()
            .add_edge(v3(0.2, 0., 0.2), v3(0.8, 0., 0.8)) // drawn inside
            .unwrap();
        vp.scene.version += 1;
        assert_eq!(drawn_edges(&mut vp), before + 1, "component: {component}");
    }
}

#[test]
fn test_the_edit_box_is_world_dashes_on_its_twelve_edges() {
    // The dashed box of the open group is geometry for the depth-tested
    // pass (so its back edges hide behind the faces), not an overlay.
    let corners = oriented_box_corners((DVec3::X, DVec3::Y, DVec3::Z), DVec3::ZERO, DVec3::ONE);
    let pts = box_dash_vertices(&corners, 0.1);
    assert!(!pts.is_empty() && pts.len() % 2 == 0);
    for p in pts {
        let on_faces = [p.x, p.y, p.z]
            .iter()
            .filter(|c| c.abs() < 1e-6 || (*c - 1.0).abs() < 1e-6)
            .count();
        assert!(on_faces >= 2); // every dash point lies on a box edge
    }
}
