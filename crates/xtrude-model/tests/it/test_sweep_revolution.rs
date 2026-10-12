// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Sweep around a circle: a profile on the axis makes a closed solid of
//! revolution.

use std::collections::HashMap;
use xtrude_core::sweep::{orient_closed_path, sweep_profile};
use xtrude_core::{DVec3, Mesh};

const N: usize = 24;

fn tau(k: usize) -> f64 {
    2.0 * std::f64::consts::PI * k as f64 / N as f64
}

fn sweep(profile: &[DVec3], path_r: f64) -> Mesh {
    let mut m = Mesh::new();
    let face = m.add_face(profile, &[]);
    let path: Vec<DVec3> = (0..N)
        .map(|k| DVec3::new(path_r * tau(k).cos(), path_r * tau(k).sin(), 0.))
        .collect();
    let oriented = orient_closed_path(&path, m.face_centroid(face), m.face_normal(face));
    assert!(sweep_profile(&mut m, face, &oriented, true));
    m
}

fn circle(r: f64, phi_deg: f64) -> Vec<DVec3> {
    let u = DVec3::new(phi_deg.to_radians().cos(), phi_deg.to_radians().sin(), 0.);
    (0..N)
        .map(|k| u * (r * tau(k).cos()) + DVec3::new(0., 0., r * tau(k).sin()))
        .collect()
}

fn valence(m: &Mesh) -> HashMap<usize, usize> {
    let mut c = HashMap::new();
    for e in m.edge_ids() {
        *c.entry(m.edge(e).faces.len()).or_insert(0) += 1;
    }
    c
}

#[test]
fn test_circle_around_a_circle_is_a_closed_sphere() {
    for phi in [0.0, 7.5, 33.0] {
        for (r, path_r) in [(1.0, 1.0), (0.5, 1.0), (1.0, 0.5)] {
            let m = sweep(&circle(r, phi), path_r);
            let worst = m
                .face_ids()
                .iter()
                .flat_map(|&f| m.face_positions(f))
                .map(|p| (p.length() - r).abs())
                .fold(0.0, f64::max);
            assert!(worst < 1e-5, "phi={phi} r={r} path_r={path_r}: {worst}");
            assert_eq!(valence(&m), HashMap::from([(2, m.edges.len())]));
            assert_eq!(m.faces.len(), N * (N / 2));
            let area: f64 = m.face_ids().iter().map(|&f| m.face_area(f)).sum();
            let sphere = 4.0 * std::f64::consts::PI * r * r;
            assert!(0.98 * sphere < area && area < sphere);
        }
    }
}

#[test]
fn test_a_profile_off_the_axis_keeps_the_mitre_sweep() {
    let ring: Vec<DVec3> = (0..N)
        .map(|k| DVec3::new(2. + 0.3 * tau(k).cos(), 0., 0.3 * tau(k).sin()))
        .collect();
    let m = sweep(&ring, 2.0);
    assert_eq!(valence(&m), HashMap::from([(2, m.edges.len())]));
    for f in m.face_ids() {
        for p in m.face_positions(f) {
            let rho = p.x.hypot(p.y);
            assert!(((rho - 2.).hypot(p.z) - 0.3).abs() < 1e-4);
        }
    }
}

#[test]
fn test_the_sphere_is_one_smooth_surface() {
    for phi in [0.0, 7.5] {
        let mut m = Mesh::new();
        let path: Vec<DVec3> = (0..N)
            .map(|k| DVec3::new(tau(k).cos(), tau(k).sin(), 0.))
            .collect();
        for i in 0..N {
            m.add_edge(path[i], path[(i + 1) % N]).unwrap();
        }
        let face = m.add_face(&circle(1.0, phi), &[]);
        let oriented = orient_closed_path(&path, m.face_centroid(face), m.face_normal(face));
        assert!(sweep_profile(&mut m, face, &oriented, true));
        let hard: Vec<_> = m
            .edge_ids()
            .into_iter()
            .filter(|&e| !m.edge(e).soft)
            .collect();
        assert!(hard.is_empty(), "phi={phi}: {} hard edges", hard.len());
    }
}
