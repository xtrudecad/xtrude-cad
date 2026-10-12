// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Pushing a region of a flat drawing while a solid stands elsewhere keeps the
//! neighbours — "solid" is a property of the region pushed, not of the whole
//! mesh (the xc.igz report).

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::viewport::Viewport;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::ids::MeshId;

fn rect(vp: &mut Viewport, x0: f64, y0: f64, x1: f64, y1: f64) {
    let pts = vec![
        v3(x0, y0, 0.),
        v3(x1, y0, 0.),
        v3(x1, y1, 0.),
        v3(x0, y1, 0.),
    ];
    let segs: Vec<_> = (0..4).map(|i| (pts[i], pts[(i + 1) % 4])).collect();
    let cmd = build_add_edges(
        &vp.scene,
        &segs,
        false,
        vec![Box::new(AddFaceCommand::new(pts))],
        None,
    );
    vp.execute_boxed(cmd);
}

fn add_box(m: &mut Mesh, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) {
    let bot = [
        v3(x0, y0, 0.),
        v3(x1, y0, 0.),
        v3(x1, y1, 0.),
        v3(x0, y1, 0.),
    ];
    let top: Vec<DVec3> = bot.iter().map(|p| v3(p.x, p.y, h)).collect();
    let rev: Vec<DVec3> = bot.iter().rev().copied().collect();
    m.add_face(&rev, &[]);
    m.add_face(&top, &[]);
    for i in 0..4 {
        let (a, b) = (bot[i], bot[(i + 1) % 4]);
        m.add_face(&[a, b, v3(b.x, b.y, h), v3(a.x, a.y, h)], &[]);
    }
}

fn area(m: &Mesh, f: FaceId) -> f64 {
    m.loop_area(&m.face(f).outer)
}

fn scene() -> Viewport {
    let mut vp = stub();
    // overlapping rectangles → three regions (the middle 2×2, area 4)
    rect(&mut vp, 0., 0., 6., 4.);
    rect(&mut vp, 4., -2., 9., 2.);
    // a separate solid box makes the whole mesh "not flat"
    add_box(vp.scene.mesh_mut(), 20., 20., 24., 24., 3.);
    vp
}

fn push_middle(vp: &mut Viewport, dist: f64) {
    let m = vp.scene.mesh();
    let middle = m
        .face_ids()
        .into_iter()
        .find(|&f| (area(m, f) - 4.0).abs() < 1e-6)
        .expect("the lens");
    let mut pp = PushPullTool::new();
    pp.begin(vp, MeshId::LOOSE, middle);
    pp.pp.extrusion = dist;
    pp.commit(vp);
}

fn flat_remainders(vp: &Viewport) -> Vec<i64> {
    let m = vp.scene.mesh();
    let mut out: Vec<i64> = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let ps = m.face_positions(f);
            ps.iter().all(|p| p.z.abs() < 1e-6) && ps[0].x < 15.0
        })
        .map(|f| (area(m, f) * 10.0).round() as i64)
        .collect();
    out.sort();
    out
}

#[test]
fn test_push_sheet_region_keeps_neighbours_despite_solid_elsewhere() {
    let mut vp = scene();
    push_middle(&mut vp, -1.0); // downward: the natural drag
    let areas = flat_remainders(&vp);
    assert!(areas.contains(&160) && areas.contains(&200), "{areas:?}"); // both survive
    let m = vp.scene.mesh();
    let low: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.z + 1.0).abs() < 1e-6))
        .collect();
    assert_eq!(low.len(), 1); // the pushed cap at z = −1
    assert!((area(m, low[0]) - 4.0).abs() < 1e-6);
}

#[test]
fn test_push_up_also_keeps_neighbours() {
    let mut vp = scene();
    push_middle(&mut vp, 1.5);
    let areas = flat_remainders(&vp);
    assert!(areas.contains(&160) && areas.contains(&200), "{areas:?}");
}
