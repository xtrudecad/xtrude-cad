// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Mass properties of closed face sets: volume, centre of mass and the
//! inertia tensor about it, per unit density.

use xtrude_core::math::DMat3;
use xtrude_core::measure::mass_properties;
use xtrude_core::{DVec3, Mesh};

/// An axis-aligned box from `lo` to `lo + size`, faces pointing out (or in
/// when `inward`).
fn cuboid(lo: DVec3, size: DVec3, inward: bool) -> Mesh {
    let (a, b, c) = (size.x, size.y, size.z);
    let p = |x: f64, y: f64, z: f64| lo + DVec3::new(x, y, z);
    let mut loops = [
        [p(0., 0., 0.), p(0., b, 0.), p(a, b, 0.), p(a, 0., 0.)],
        [p(0., 0., c), p(a, 0., c), p(a, b, c), p(0., b, c)],
        [p(0., 0., 0.), p(a, 0., 0.), p(a, 0., c), p(0., 0., c)],
        [p(a, 0., 0.), p(a, b, 0.), p(a, b, c), p(a, 0., c)],
        [p(a, b, 0.), p(0., b, 0.), p(0., b, c), p(a, b, c)],
        [p(0., b, 0.), p(0., 0., 0.), p(0., 0., c), p(0., b, c)],
    ];
    if inward {
        for lp in loops.iter_mut() {
            lp.reverse();
        }
    }
    let mut m = Mesh::new();
    for lp in loops {
        m.add_face(&lp, &[]);
    }
    m
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1e-30)
}

fn assert_diagonal(t: &DMat3, want: DVec3) {
    let cols = [t.x_axis, t.y_axis, t.z_axis];
    for (i, col) in cols.iter().enumerate() {
        for j in 0..3 {
            let w = if i == j { want[i] } else { 0.0 };
            assert!(
                (col[j] - w).abs() <= 1e-9 * want.max_element(),
                "inertia[{i}][{j}] = {}, want {w}",
                col[j]
            );
        }
    }
}

#[test]
fn test_a_cube_has_its_volume_centre_and_inertia() {
    let a = 0.02;
    let m = cuboid(DVec3::ZERO, DVec3::splat(a), false);
    let mp = mass_properties(&m, &m.face_ids()).unwrap();
    let v = a * a * a;
    assert!(close(mp.volume, v), "{}", mp.volume);
    assert!(mp.centre.abs_diff_eq(DVec3::splat(a / 2.0), 1e-15));
    // m·a²/6 on the diagonal, nothing off it.
    assert_diagonal(&mp.inertia, DVec3::splat(v * a * a / 6.0));
}

#[test]
fn test_a_box_has_m_b2_c2_over_12_on_the_diagonal() {
    let (a, b, c) = (0.01, 0.03, 0.05);
    let m = cuboid(DVec3::ZERO, DVec3::new(a, b, c), false);
    let mp = mass_properties(&m, &m.face_ids()).unwrap();
    let v = a * b * c;
    assert!(close(mp.volume, v));
    assert!(mp.centre.abs_diff_eq(DVec3::new(a, b, c) / 2.0, 1e-15));
    assert_diagonal(
        &mp.inertia,
        DVec3::new(
            v * (b * b + c * c) / 12.0,
            v * (a * a + c * c) / 12.0,
            v * (a * a + b * b) / 12.0,
        ),
    );
}

#[test]
fn test_a_translated_box_moves_its_centre_not_its_inertia() {
    let size = DVec3::new(0.01, 0.03, 0.05);
    let at = DVec3::new(1.5, -0.7, 0.25);
    let here = cuboid(DVec3::ZERO, size, false);
    let there = cuboid(at, size, false);
    let a = mass_properties(&here, &here.face_ids()).unwrap();
    let b = mass_properties(&there, &there.face_ids()).unwrap();
    assert!(close(b.volume, a.volume));
    assert!(b.centre.abs_diff_eq(at + size / 2.0, 1e-12));
    let v = a.volume;
    assert_diagonal(
        &b.inertia,
        DVec3::new(
            v * (size.y * size.y + size.z * size.z) / 12.0,
            v * (size.x * size.x + size.z * size.z) / 12.0,
            v * (size.x * size.x + size.y * size.y) / 12.0,
        ),
    );
}

#[test]
fn test_faces_pointing_inwards_give_the_same_properties() {
    let size = DVec3::new(0.01, 0.03, 0.05);
    let out = cuboid(DVec3::ONE, size, false);
    let inw = cuboid(DVec3::ONE, size, true);
    let a = mass_properties(&out, &out.face_ids()).unwrap();
    let b = mass_properties(&inw, &inw.face_ids()).unwrap();
    assert!(close(b.volume, a.volume));
    assert!(b.centre.abs_diff_eq(a.centre, 1e-12));
    assert!(b.inertia.abs_diff_eq(a.inertia, 1e-18));
}

#[test]
fn test_an_open_set_has_no_mass_properties() {
    let m = cuboid(DVec3::ZERO, DVec3::splat(0.02), false);
    let mut faces = m.face_ids();
    faces.pop();
    assert!(mass_properties(&m, &faces).is_none());
    assert!(mass_properties(&m, &[]).is_none());
}
