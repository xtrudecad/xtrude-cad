// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Concentric rings on a slab pushed ring by ring land with the exact volume;
//! a push the guard refuses tops out at the reachable height;
//! nested holes are dropped.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::TAU;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::pushpull::{last_distance, PushPullTool};
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::orient::is_closed;
use xtrude_core::triangulate::triangulate;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::{AddFaceCommand, RebuildPlanarFacesCommand, TagCurveCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::ids::MeshId;

fn circle(r: f64) -> Vec<DVec3> {
    (0..24)
        .map(|i| {
            let a = TAU * i as f64 / 24.0;
            v3(r * a.cos(), r * a.sin(), 0.)
        })
        .collect()
}

fn setup() -> Viewport {
    let mut vp = stub();
    let sq = [
        v3(-8., -8., 0.),
        v3(8., -8., 0.),
        v3(8., 8., 0.),
        v3(-8., 8., 0.),
    ];
    let segs: Vec<_> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let cmd = build_add_edges(
        &vp.scene,
        &segs,
        true,
        vec![Box::new(AddFaceCommand::new(sq.to_vec()))],
        None,
    );
    vp.execute_boxed(cmd);
    for r in [5.0, 3.0, 1.5] {
        let pts = circle(r);
        let segs: Vec<_> = (0..24).map(|i| (pts[i], pts[(i + 1) % 24])).collect();
        let cmd = build_add_edges(
            &vp.scene,
            &segs,
            false,
            vec![
                Box::new(TagCurveCommand {
                    pts: pts.clone(),
                    closed: true,
                }),
                Box::new(RebuildPlanarFacesCommand::new()),
            ],
            None,
        );
        vp.execute_boxed(cmd);
    }
    vp
}

fn push(vp: &mut Viewport, face: FaceId, dist: f64) {
    let mut pp = PushPullTool::new();
    pp.begin(vp, MeshId::LOOSE, face);
    pp.pp.extrusion = dist;
    pp.commit(vp);
}

fn at0(m: &Mesh, f: FaceId) -> bool {
    m.face_positions(f).iter().all(|p| p.z.abs() < 1e-6)
}

/// The outer loop's area.
fn area(m: &Mesh, f: FaceId) -> f64 {
    m.loop_area(&m.face(f).outer)
}

fn volume(m: &Mesh) -> f64 {
    let mut v = 0.0;
    for f in m.face_ids() {
        for [a, b, c] in triangulate(
            &m.face_positions(f),
            &m.face_hole_positions(f),
            Some(m.face_normal(f)),
        ) {
            v += a.dot(b.cross(c)) / 6.0;
        }
    }
    v
}

fn messages(vp: &Viewport) -> Vec<String> {
    vp.outbox
        .events
        .iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t.to_lowercase()),
            _ => None,
        })
        .collect()
}

fn ring_with_hole(m: &Mesh, outer_area: Option<f64>, sides: Option<usize>) -> FaceId {
    m.face_ids()
        .into_iter()
        .find(|&f| {
            at0(m, f)
                && !m.face(f).holes.is_empty()
                && sides.is_none_or(|n| m.face(f).outer.len() == n)
                && outer_area.is_none_or(|a| (area(m, f) - a).abs() < 2.0)
        })
        .expect("the ring")
}

#[test]
fn test_ring_pushes_between_levels_land_exactly() {
    let mut vp = setup();
    let slab = ring_with_hole(vp.scene.mesh(), None, Some(4));
    push(&mut vp, slab, 2.0);
    let v0 = volume(vp.scene.mesh());
    let ring1 = ring_with_hole(vp.scene.mesh(), Some(77.65), None);
    let a1 = area(vp.scene.mesh(), ring1) - 27.95; // r5 minus its r3 hole
    push(&mut vp, ring1, 1.0);
    assert!(is_closed(vp.scene.mesh()));
    assert!((volume(vp.scene.mesh()) - (v0 + a1)).abs() < 2e-2);
    let ring2 = ring_with_hole(vp.scene.mesh(), Some(27.95), None);
    let a2 = area(vp.scene.mesh(), ring2) - 6.99; // r3 minus its r1.5 hole
    let v1 = volume(vp.scene.mesh());
    push(&mut vp, ring2, 0.5);
    assert!(is_closed(vp.scene.mesh()));
    assert!((volume(vp.scene.mesh()) - (v1 + a2 * 0.5)).abs() < 2e-2);
    assert!(!messages(&vp).iter().any(|m| m.contains("refused")));
    // No wall stays inside: bar the membranes across the innermost hole.
    let m = vp.scene.mesh();
    let mut by_count: HashMap<usize, usize> = HashMap::new();
    for (_, e) in m.edges.iter() {
        *by_count.entry(e.faces.len()).or_default() += 1;
    }
    assert_eq!(by_count.get(&3).copied().unwrap_or(0), 24);
}

#[test]
fn test_refused_push_tops_out_at_the_reachable_height() {
    let mut vp = stub();
    for r in [5.0, 3.0, 1.5] {
        let mut t = RadialTool::circle();
        for x in [0.0, r] {
            t.on_click(&mut vp, &ToolContext::at(v3(x, 0., 0.)));
        }
    }
    let face_by_area = |vp: &Viewport, a: f64| -> FaceId {
        let m = vp.scene.mesh();
        m.face_ids()
            .into_iter()
            .find(|&f| (area(m, f) - a).abs() < 0.6 && at0(m, f))
            .expect("the face")
    };
    let outer = face_by_area(&vp, 77.6);
    push(&mut vp, outer, 2.0); // the outer ring up 2
    vp.outbox.events.clear();
    let iris = face_by_area(&vp, 28.0);
    let mut pp = PushPullTool::new();
    pp.begin(&mut vp, MeshId::LOOSE, iris);
    let faces_before = vp.scene.mesh().face_ids().len();
    for d in [1.5, 1.9, 2.0, 2.4] {
        pp.pp.extrusion = d;
        pp.show_light_preview(&mut vp);
        assert_eq!(vp.scene.mesh().face_ids().len(), faces_before); // overlay only
    }
    pp.pp.extrusion = 2.4;
    pp.commit(&mut vp);
    let m = vp.scene.mesh();
    assert!(is_closed(m)); // never commits a crack
    assert_ne!(m.face_ids().len(), faces_before); // ...and it DID something
    let last = last_distance().expect("a distance");
    assert!(0.0 < last && last < 2.4, "{last}"); // topped out below the level
    let zs: BTreeSet<i64> = m
        .verts
        .iter()
        .map(|(_, x)| (x.position.z * 1e4).round() as i64)
        .collect();
    assert!(zs.iter().any(|&z| (z as f64 / 1e4 - last).abs() < 1e-3));
    assert!(messages(&vp).iter().any(|m| m.contains("stopped")));
}

#[test]
fn test_add_face_drops_nested_holes() {
    let mut m = Mesh::new();
    let outer: Vec<DVec3> = [(-4., -4.), (4., -4.), (4., 4.), (-4., 4.)]
        .iter()
        .map(|&(x, y)| v3(x, y, 2.))
        .collect();
    let lift = |lp: Vec<DVec3>| lp.into_iter().map(|p| v3(p.x, p.y, 2.)).collect::<Vec<_>>();
    let face = m.add_face(&outer, &[lift(circle(2.0)), lift(circle(0.8))]);
    assert_eq!(m.face(face).holes.len(), 1); // the nested r=0.8 dropped
    let hole_r: BTreeSet<i64> = m.face(face).holes[0]
        .iter()
        .map(|&v| {
            let p = m.pos(v);
            (p.x.hypot(p.y) * 10.0).round() as i64
        })
        .collect();
    assert_eq!(hole_r.into_iter().collect::<Vec<_>>(), vec![20]);
    // earcut stays sane: no triangle lands inside the opening
    for [a, b, c] in triangulate(
        &m.face_positions(face),
        &m.face_hole_positions(face),
        Some(m.face_normal(face)),
    ) {
        let cen = (a + b + c) / 3.0;
        assert!(cen.x.hypot(cen.y) > 2.0 - 1e-6);
    }
}
