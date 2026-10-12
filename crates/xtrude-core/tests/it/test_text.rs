// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3D Text geometry (the text-label tests live with the document and the app).

use std::collections::HashMap;
use xtrude_core::orient::signed_volume;
use xtrude_core::text3d::build_text_mesh;
use xtrude_core::{Mesh, VertexId};

fn closed(mesh: &Mesh) -> bool {
    let mut counts: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for (_, f) in mesh.faces.iter() {
        for ring in f.loops() {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                *counts
                    .entry(if a < b { (a, b) } else { (b, a) })
                    .or_insert(0) += 1;
            }
        }
    }
    !counts.is_empty() && counts.values().all(|&c| c == 2)
}

#[test]
fn test_3d_text_builds_watertight_letters_with_holes() {
    let mesh = build_text_mesh("O", "Sans", false, false, 0.5, 0.1);
    assert!(!mesh.faces.is_empty());
    assert!(closed(&mesh));
    assert!(signed_volume(&mesh) > 0.0);
    assert!(mesh.faces.iter().any(|(_, f)| !f.holes.is_empty()));
}

#[test]
fn test_3d_text_height_is_the_real_block_height() {
    let mesh = build_text_mesh("AB", "Sans", false, false, 0.5, 0.05);
    let zs: Vec<f64> = mesh.verts.iter().map(|(_, v)| v.position.z).collect();
    let (lo, hi) = zs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &z| (a.min(z), b.max(z)));
    assert!(((hi - lo) - 0.5).abs() < 1e-6);
    assert!(lo.abs() < 1e-9);
}

#[test]
fn test_3d_text_flat_when_thickness_zero() {
    let mesh = build_text_mesh("I", "Sans", false, false, 0.3, 0.0);
    assert!(!mesh.faces.is_empty());
    let ys: Vec<f64> = mesh.verts.iter().map(|(_, v)| v.position.y).collect();
    let (lo, hi) = ys
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &y| (a.min(y), b.max(y)));
    assert!((hi - lo).abs() < 1e-9);
}

#[test]
fn test_3d_text_empty_string_builds_nothing() {
    assert!(build_text_mesh("", "Sans", false, false, 0.25, 0.05)
        .faces
        .is_empty());
    assert!(build_text_mesh("   ", "Sans", false, false, 0.25, 0.05)
        .faces
        .is_empty());
}

#[test]
fn test_3d_text_curve_seams_are_soft() {
    let mesh = build_text_mesh("O", "Sans", false, false, 0.5, 0.1);
    let soft: Vec<_> = mesh.edges.iter().filter(|(_, e)| e.soft).collect();
    assert!(!soft.is_empty());
    for (_, e) in &soft {
        for &f in &e.faces {
            assert!(mesh.face_normal(f).y.abs() < 0.5);
        }
    }
    let hard_outline = mesh.edges.iter().any(|(_, e)| {
        e.faces.len() == 2 && !e.soft && e.faces.iter().any(|&f| mesh.face_normal(f).y.abs() > 0.5)
    });
    assert!(hard_outline);
}
