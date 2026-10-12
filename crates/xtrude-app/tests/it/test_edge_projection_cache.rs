// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The edge picks project the indexed edges once per view: the hovers
//! that follow while the camera stays still reuse the projections, a
//! camera or scene change projects again, and the picks answer exactly
//! what projecting each point through the probe answers.

use std::rc::Rc;

use crate::common;

use common::{hover, sized_viewport, v3, FakeProbe};
use glam::{DMat4, DVec3};
use xtrude_app::tools::select::SelectTool;
use xtrude_app::viewport::{CameraProbe, Projection, Viewport};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

/// The six faces of an axis-aligned unit box: twelve hard edges.
fn unit_box() -> Mesh {
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
        let pts: Vec<DVec3> = idx.iter().map(|&i| p[i]).collect();
        m.add_face(&pts, &[]);
    }
    m
}

/// Twelve placements of one box component on a grid, a loose square
/// beside them, seen in iso with the Select tool active.
fn field() -> Viewport {
    let mut vp = sized_viewport(800., 600.);
    let proto = add_mesh(&mut vp.scene.doc.meshes, unit_box());
    for i in 0..4 {
        for j in 0..3 {
            let mut g = Group::new(proto, None);
            g.xform = Some(DMat4::from_translation(v3(
                i as f64 * 3.,
                j as f64 * 3.,
                0.,
            )));
            vp.scene.doc.groups.push(g);
        }
    }
    vp.scene.mesh_mut().add_face(
        &[
            v3(-4., 0., 0.),
            v3(-2., 0., 0.),
            v3(-2., 2., 0.),
            v3(-4., 2., 0.),
        ],
        &[],
    );
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(SelectTool::new())));
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-4., 0., 0.), v3(10., 7., 1.), 1.2);
    vp
}

#[test]
fn test_hovers_reuse_the_projections_until_the_view_changes() {
    let mut vp = field();
    // the background: the face pick misses, the edge fallback runs
    hover(&mut vp, 3., 3.);
    let passes = vp.edge_projection_passes();
    assert!(passes > 0, "the hover projects the edges");
    hover(&mut vp, 5., 4.);
    hover(&mut vp, 400., 300.);
    assert_eq!(
        vp.edge_projection_passes(),
        passes,
        "hovers under the same view reuse the projections"
    );

    vp.camera.orbit(10.0, 0.0, 600);
    hover(&mut vp, 3., 3.);
    let after_orbit = vp.edge_projection_passes();
    assert!(after_orbit > passes, "a camera change projects again");
    hover(&mut vp, 6., 3.);
    assert_eq!(vp.edge_projection_passes(), after_orbit);

    vp.resize(700., 600.);
    vp.camera.set_aspect(700, 600);
    hover(&mut vp, 3., 3.);
    let after_resize = vp.edge_projection_passes();
    assert!(after_resize > after_orbit, "a resize projects again");

    vp.scene.doc.groups.pop();
    vp.scene.version += 1;
    hover(&mut vp, 3., 3.);
    assert!(
        vp.edge_projection_passes() > after_resize,
        "a scene change projects again"
    );
}

#[test]
fn test_a_probe_without_projection_is_never_cached() {
    let mut vp = field();
    let cam = vp.camera.clone();
    let (w, h) = (vp.width(), vp.height());
    FakeProbe::of(&mut vp).world_to_pixel(move |p| {
        let pr = Projection {
            mvp: cam.projection_matrix() * cam.view_matrix(),
            width: w,
            height: h,
        };
        pr.to_pixel(p).map(|s| s.tuple())
    });
    vp.pick_edge_any(3., 3.);
    let passes = vp.edge_projection_passes();
    vp.pick_edge_any(3., 3.);
    assert!(
        vp.edge_projection_passes() > passes,
        "point-by-point projections may change between calls"
    );
}

/// What the edge picks answer at `(x, y)`, with the group pick only where
/// no face is under the cursor (there it is the edge fallback).
fn edge_answers(vp: &mut Viewport, x: f64, y: f64, background: bool) -> String {
    let group = background.then(|| vp.pick_group(x, y));
    format!(
        "{group:?} {:?} {:?} {:?} {:?}",
        vp.pick_edge_any(x, y),
        vp.pick_vertex(x, y),
        vp.nearby_group_edges(x, y, 20., 4)
            .iter()
            .map(|e| (e.a, e.b, e.group))
            .collect::<Vec<_>>(),
        vp.nearby_loose_edges(x, y, 20., 4),
    )
}

#[test]
fn test_the_cached_projections_pick_what_point_projections_pick() {
    let mut vp = field();
    let mut points = Vec::new();
    for i in 0..40 {
        for j in 0..30 {
            points.push((i as f64 * 20. + 3., j as f64 * 20. + 7.));
        }
    }
    let background: Vec<bool> = points
        .iter()
        .map(|&(x, y)| vp.pick_face_any(x, y).is_none())
        .collect();
    let cached: Vec<String> = points
        .iter()
        .zip(&background)
        .map(|(&(x, y), &bg)| edge_answers(&mut vp, x, y, bg))
        .collect();
    assert!(
        cached.iter().any(|a| a.starts_with("Some(Some(")),
        "some background point picks a group by its edges"
    );

    // the same view, projected point by point through the probe
    let cam = vp.camera.clone();
    let (w, h) = (vp.width(), vp.height());
    FakeProbe::of(&mut vp).world_to_pixel(move |p| {
        let pr = Projection {
            mvp: cam.projection_matrix() * cam.view_matrix(),
            width: w,
            height: h,
        };
        pr.to_pixel(p).map(|s| s.tuple())
    });
    let per_point: Vec<String> = points
        .iter()
        .zip(&background)
        .map(|(&(x, y), &bg)| edge_answers(&mut vp, x, y, bg))
        .collect();
    assert_eq!(cached, per_point);

    vp.set_probe(Rc::new(CameraProbe));
    let again: Vec<String> = points
        .iter()
        .zip(&background)
        .map(|(&(x, y), &bg)| edge_answers(&mut vp, x, y, bg))
        .collect();
    assert_eq!(cached, again);
}

#[test]
fn test_a_background_hover_picks_the_group_beside_its_edge() {
    let mut vp = field();
    // just outside the near bottom edge of the first box
    let (a, b) = (v3(0., 0., 0.), v3(1., 0., 0.));
    let mid = vp.world_to_pixel((a + b) * 0.5).expect("on screen");
    let below = (mid.x, mid.y + 3.);
    assert!(
        vp.pick_face_any(below.0, below.1).is_none(),
        "the background"
    );
    let first = vp.scene.doc.groups[0].id;
    assert_eq!(vp.pick_group(below.0, below.1), Some(first));
    hover(&mut vp, below.0, below.1);
    assert_eq!(vp.pick_group(below.0, below.1), Some(first));
}
