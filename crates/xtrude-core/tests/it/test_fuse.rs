// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Coplanar fusion of imported triangles.

use xtrude_core::attrs::FaceAttrs;
use xtrude_core::fuse::{fuse_coplanar_loops, soften_smooth_edges, Loop};
use xtrude_core::{DVec3, Mesh};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn grid(nx: usize, ny: usize, attrs: Option<FaceAttrs>) -> Vec<Loop> {
    let mut out = Vec::new();
    for i in 0..nx {
        for j in 0..ny {
            let (i, j) = (i as f64, j as f64);
            let (a, b, c, d) = (
                v(i, j, 0.),
                v(i + 1., j, 0.),
                v(i + 1., j + 1., 0.),
                v(i, j + 1., 0.),
            );
            out.push((vec![a, b, c], attrs.clone()));
            out.push((vec![a, c, d], attrs.clone()));
        }
    }
    out
}

#[test]
fn test_grid_fuses_to_one_face() {
    let fused = fuse_coplanar_loops(&grid(4, 3, None), 0.99999, false);
    assert_eq!(fused.len(), 1);
    let (outer, holes, _attrs, originals) = &fused[0];
    assert!(holes.is_empty());
    assert_eq!(originals.len(), 24);
    assert!(outer
        .iter()
        .all(|p| p.x == 0.0 || p.x == 4.0 || p.y == 0.0 || p.y == 3.0));
}

#[test]
fn test_hole_is_traced() {
    let loops: Vec<Loop> = grid(3, 3, None)
        .into_iter()
        .filter(|t| {
            !t.0.iter()
                .all(|p| (1.0..=2.0).contains(&p.x) && (1.0..=2.0).contains(&p.y))
        })
        .collect();
    let fused = fuse_coplanar_loops(&loops, 0.99999, false);
    assert_eq!(fused.len(), 1);
    let (_outer, holes, _a, _o) = &fused[0];
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].len(), 4);
}

#[test]
fn test_material_boundary_splits_regions() {
    let red = FaceAttrs {
        paint: xtrude_core::attrs::Paint {
            color: Some([1., 0., 0.]),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut loops = grid(2, 1, None);
    loops.push((
        vec![v(2., 0., 0.), v(3., 0., 0.), v(3., 1., 0.)],
        Some(red.clone()),
    ));
    loops.push((vec![v(2., 0., 0.), v(3., 1., 0.), v(2., 1., 0.)], Some(red)));
    let fused = fuse_coplanar_loops(&loops, 0.99999, false);
    assert_eq!(fused.len(), 2);
    let painted: std::collections::HashSet<bool> = fused
        .iter()
        .map(|f| f.2.as_ref().is_some_and(|a| a.paint.color.is_some()))
        .collect();
    let mut sizes: Vec<usize> = fused.iter().map(|f| f.3.len()).collect();
    sizes.sort();
    assert!(painted == [false, true].into_iter().collect() || sizes == [2, 4]);
}

#[test]
fn test_crease_is_not_merged() {
    let loops: Vec<Loop> = vec![
        (vec![v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.)], None),
        (vec![v(0., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)], None),
        (vec![v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.)], None),
        (vec![v(0., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)], None),
    ];
    let fused = fuse_coplanar_loops(&loops, 0.99999, false);
    assert_eq!(fused.len(), 2);
    assert!(fused.iter().all(|f| f.3.len() == 2));
}

#[test]
fn test_soften_smooth_edges_marks_shallow_dihedrals() {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v(0., 0., 0.),
            v(1., 0., 0.),
            v(1., 1., 0.05),
            v(0., 1., 0.05),
        ],
        &[],
    );
    m.add_face(
        &[
            v(0., 1., 0.05),
            v(1., 1., 0.05),
            v(1., 2., 0.0),
            v(0., 2., 0.0),
        ],
        &[],
    );
    m.add_face(
        &[v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)],
        &[],
    );
    soften_smooth_edges(&mut m, 0.85);
    let soft: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| m.edge(e).soft)
        .collect();
    assert_eq!(soft.len(), 1);
    let (a, b) = m.edge_ab(soft[0]);
    assert!((a.y - 1.0).abs() < 0.005 && (b.y - 1.0).abs() < 0.005);
}
