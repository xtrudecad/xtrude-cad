// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Polygon offset, and the ring/inner face split the Offset tool builds (walls
//! with thickness).

use crate::common;

use common::*;
use std::collections::BTreeSet;
use xtrude_core::orient::is_closed;
use xtrude_core::topology::offset_loop;
use xtrude_core::DVec3;
use xtrude_model::commands::{AddFaceCommand, DeleteFaceCommand};
use xtrude_model::history::CompoundCommand;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::PushPull;

fn r2(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

fn xy_keys(ps: &[DVec3]) -> BTreeSet<(i64, i64)> {
    ps.iter().map(|p| (r2(p.x), r2(p.y))).collect()
}

fn keys(pts: &[(f64, f64)]) -> BTreeSet<(i64, i64)> {
    pts.iter().map(|&(x, y)| (r2(x), r2(y))).collect()
}

fn square4() -> Vec<DVec3> {
    vec![V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)]
}

// ---- offset_loop -------------------------------------------------------------

#[test]
fn test_offset_inward_shrinks_rectangle() {
    let off = offset_loop(&square4(), DVec3::Z, 0.5).unwrap();
    assert_eq!(
        xy_keys(&off),
        keys(&[(0.5, 0.5), (3.5, 0.5), (3.5, 3.5), (0.5, 3.5)])
    );
}

#[test]
fn test_offset_outward_grows_rectangle() {
    let off = offset_loop(&square4(), DVec3::Z, -0.5).unwrap();
    assert_eq!(
        xy_keys(&off),
        keys(&[(-0.5, -0.5), (4.5, -0.5), (4.5, 4.5), (-0.5, 4.5)])
    );
}

#[test]
fn test_offset_overshoot_rejected() {
    assert!(offset_loop(&square4(), DVec3::Z, 2.0).is_none()); // exactly collapses
    assert!(offset_loop(&square4(), DVec3::Z, 3.0).is_none()); // crosses
}

#[test]
fn test_offset_on_vertical_wall_plane() {
    let wall = [
        V3(0., 0., 0.),
        V3(4., 0., 0.),
        V3(4., 0., 3.),
        V3(0., 0., 3.),
    ];
    let off = offset_loop(&wall, -DVec3::Y, 0.5).unwrap();
    assert!(off.iter().all(|p| p.y.abs() < 1e-9)); // stays in the plane
    let zs: BTreeSet<i64> = off.iter().map(|p| r2(p.z)).collect();
    assert_eq!(zs.into_iter().collect::<Vec<_>>(), vec![50, 250]);
}

// ---- the ring / inner split (what OffsetTool._commit builds) ------------------

#[test]
fn test_offset_split_makes_ring_and_inner() {
    let (mut scene, mut hist) = scene_history();
    let face = scene.mesh_mut().add_face(&square4(), &[]);
    let lp = scene.mesh().face_positions(face);
    let inner = offset_loop(&lp, DVec3::Z, 1.0).unwrap();
    exec(
        &mut scene,
        &mut hist,
        Box::new(CompoundCommand::new(vec![
            Box::new(DeleteFaceCommand { lp }),
            Box::new(AddFaceCommand::plain(square4(), vec![inner.clone()], None)),
            Box::new(AddFaceCommand::plain(inner.clone(), vec![], None)),
        ])),
    );
    let m = scene.mesh();
    let ids = m.face_ids();
    assert_eq!(ids.len(), 2);
    let ring = *ids.iter().find(|&&f| !m.face(f).holes.is_empty()).unwrap();
    let inner_face = *ids.iter().find(|&&f| m.face(f).holes.is_empty()).unwrap();
    assert_eq!(m.face(ring).holes.len(), 1); // the ring is an annulus
                                             // the OUTER boundary's area
    assert!((m.loop_area(&m.face(ring).outer) - 16.0).abs() < 1e-6);
    assert!((m.face_area(inner_face) - 4.0).abs() < 1e-6); // the 2×2 room
    let hole: Vec<DVec3> = m.face(ring).holes[0].iter().map(|&v| m.pos(v)).collect();
    let want = keys(&[(1., 1.), (3., 1.), (3., 3.), (1., 3.)]);
    assert_eq!(xy_keys(&hole), want);
    assert_eq!(xy_keys(&m.face_positions(inner_face)), want);

    assert!(hist.undo(&mut scene));
    let m = scene.mesh();
    assert_eq!(m.face_ids().len(), 1);
    assert!((m.face_area(m.face_ids()[0]) - 16.0).abs() < 1e-6);
}

#[test]
fn test_pushing_an_offset_ring_raises_walls_with_thickness() {
    let (mut scene, _hist) = scene_history();
    let outer = square4();
    let inner = offset_loop(&outer, DVec3::Z, 0.15).unwrap();
    let m = scene.mesh_mut();
    m.add_face(&inner, &[]); // room floor
    let ring = m.add_face(&outer, std::slice::from_ref(&inner)); // wall footprint (annulus)

    let mut pp = PushPull::new();
    pp.setup_plain(scene.mesh(), MeshId::LOOSE, ring);
    pp.extrusion = 3.0;
    pp.prism_cap = false;
    pp.limit_in = None;
    pp.mutate(&mut scene);

    // Watertight, the floor stays, the cap is a holed annulus up top, and
    // both wall rings rose: 4 + 4 wall quads.
    let m = scene.mesh();
    assert!(is_closed(m));
    let ids = m.face_ids();
    assert!(ids.iter().any(
        |&f| m.face(f).holes.is_empty() && m.face_positions(f).iter().all(|p| p.z.abs() < 1e-6)
    ));
    let caps = ids
        .iter()
        .filter(|&&f| {
            !m.face(f).holes.is_empty()
                && m.face_positions(f).iter().all(|p| (p.z - 3.0).abs() < 1e-6)
        })
        .count();
    assert_eq!(caps, 1);
    let walls = ids
        .iter()
        .filter(|&&f| {
            let zs: BTreeSet<i64> = m
                .face_positions(f)
                .iter()
                .map(|p| (p.z * 10.0).round() as i64)
                .collect();
            zs.into_iter().collect::<Vec<_>>() == vec![0, 30]
        })
        .count();
    assert_eq!(walls, 8);
}
