// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Merge planar regions of a triangulated mesh, in any orientation.

use crate::common;
use common::*;
use xtrude_core::fuse::simplify_mesh;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::SimplifyMeshCommand;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

fn grid(plane: &str, width: usize, height: usize) -> Mesh {
    let point = |x: f64, y: f64| match plane {
        "XY" => V3(x, y, 0.),
        "XZ" => V3(x, 0., y),
        "YZ" => V3(0., x, y),
        _ => V3(x, y, x + y),
    };
    let mut m = Mesh::new();
    for x in 0..width {
        for y in 0..height {
            let (x, y) = (x as f64, y as f64);
            let (a, b, c, d) = (
                point(x, y),
                point(x + 1., y),
                point(x + 1., y + 1.),
                point(x, y + 1.),
            );
            m.add_face(&[a, b, c], &[]);
            m.add_face(&[a, c, d], &[]);
        }
    }
    m
}

#[test]
fn test_simplify_mesh_merges_planar_regions_in_any_orientation() {
    for plane in ["XY", "XZ", "YZ", "tilted"] {
        let mut m = grid(plane, 12, 8);
        let removed = simplify_mesh(&mut m, 0.0);
        assert_eq!(removed, 12 * 8 * 2 - 1, "{plane}");
        assert_eq!(m.faces.len(), 1);
        assert_eq!(m.verts.len(), 4);
    }
}

#[test]
fn test_angle_tolerance_simplifies_gently_rounded_facets() {
    let mut m = Mesh::new();
    let segments = 90;
    let radius = 10.0;
    for i in 0..segments {
        let a0 = std::f64::consts::PI * i as f64 / (2 * segments) as f64;
        let a1 = std::f64::consts::PI * (i + 1) as f64 / (2 * segments) as f64;
        let a = V3(radius * a0.cos(), radius * a0.sin(), 0.);
        let b = V3(radius * a1.cos(), radius * a1.sin(), 0.);
        let c = V3(radius * a1.cos(), radius * a1.sin(), 2.);
        let d = V3(radius * a0.cos(), radius * a0.sin(), 2.);
        m.add_face(&[a, b, c], &[]);
        m.add_face(&[a, c, d], &[]);
    }
    let before: Vec<DVec3> = m.verts.iter().map(|(_, v)| v.position).collect();
    let removed = simplify_mesh(&mut m, 1.0);
    assert!(removed > segments);
    assert!(m.faces.len() < segments);
    let max = |f: fn(&DVec3) -> f64| {
        m.verts
            .iter()
            .map(|(_, v)| f(&v.position))
            .fold(f64::MIN, f64::max)
    };
    assert!((max(|p| p.x) - radius).abs() < 1e-6);
    assert!((max(|p| p.y) - radius).abs() < 1e-6);
    assert!((max(|p| p.z) - 2.0).abs() < 1e-6);
    assert!(m.verts.iter().all(|(_, v)| before.contains(&v.position)));
}

#[test]
fn test_simplify_preserves_face_attributes_and_holes() {
    let mut m = Mesh::new();
    let (a, b, c, d) = (
        V3(0., 0., 0.),
        V3(1., 0., 0.),
        V3(1., 1., 0.),
        V3(0., 1., 0.),
    );
    let first = m.add_face(&[a, b, c], &[]);
    let second = m.add_face(&[a, c, d], &[]);
    m.face_mut(first).attrs.layer = Some("one".into());
    m.face_mut(second).attrs.layer = Some("two".into());
    simplify_mesh(&mut m, 0.0);
    assert_eq!(m.faces.len(), 2);
    let layers: std::collections::HashSet<_> = m
        .face_ids()
        .iter()
        .map(|&f| m.face(f).attrs.layer.clone())
        .collect();
    assert_eq!(
        layers,
        [Some("one".to_string()), Some("two".to_string())]
            .into_iter()
            .collect()
    );

    let mut holed = Mesh::new();
    let f = holed.add_face(
        &[a, b, c, d],
        &[vec![
            V3(0.25, 0.25, 0.),
            V3(0.75, 0.25, 0.),
            V3(0.75, 0.75, 0.),
            V3(0.25, 0.75, 0.),
        ]],
    );
    holed.face_mut(f).interior = true;
    simplify_mesh(&mut holed, 0.0);
    assert_eq!(holed.faces.len(), 1);
    let f = holed.face_ids()[0];
    assert_eq!(holed.face(f).holes.len(), 1);
    assert!(holed.face(f).interior);
}

#[test]
fn test_simplify_mesh_command_can_be_undone_and_redone() {
    let mut s = Scene::new();
    s.doc
        .meshes
        .insert(MeshId::LOOSE, std::sync::Arc::new(grid("XZ", 12, 8)));
    let mut h = xtrude_model::history::History::new();
    let original = s.mesh().clone();
    let mut cmd = SimplifyMeshCommand {
        max_angle_degrees: 0.0,
        faces_removed: 0,
    };
    h.execute(&mut s, &mut cmd);
    assert!(h.last_error.is_none());
    assert!(cmd.faces_removed > 0);
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().face_ids(), original.face_ids());
    assert!(s
        .mesh()
        .face_ids()
        .iter()
        .all(|&f| s.mesh().face_positions(f) == original.face_positions(f)));
    assert!(h.redo(&mut s));
    assert_eq!(s.mesh().faces.len(), 1);
}
