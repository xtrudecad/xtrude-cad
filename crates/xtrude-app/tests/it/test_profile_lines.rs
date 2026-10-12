// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Profiles: a soft edge is drawn when the eye sees one of its faces from
//! the front and the other from the back (or when it bounds one face
//! only). Checked on placed soft-sided cylinders from several eyes, frame
//! after frame, against the rule worked out here.

use crate::common;

use common::v3;
use glam::{DMat4, DVec3};
use std::collections::BTreeSet;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

const SIDES: usize = 16;
const RADIUS: f64 = 0.4;

fn corner(i: usize, z: f64) -> DVec3 {
    let a = std::f64::consts::TAU * (i % SIDES) as f64 / SIDES as f64;
    DVec3::new(a.cos() * RADIUS, a.sin() * RADIUS, z)
}

/// A closed cylinder whose vertical edges are soft.
fn cylinder() -> Mesh {
    let mut m = Mesh::new();
    let bottom: Vec<DVec3> = (0..SIDES).rev().map(|i| corner(i, 0.)).collect();
    let top: Vec<DVec3> = (0..SIDES).map(|i| corner(i, 1.)).collect();
    m.add_face(&bottom, &[]);
    m.add_face(&top, &[]);
    for i in 0..SIDES {
        m.add_face(
            &[
                corner(i, 0.),
                corner(i + 1, 0.),
                corner(i + 1, 1.),
                corner(i, 1.),
            ],
            &[],
        );
    }
    let soft: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.z - b.z).abs() > 0.5
        })
        .collect();
    for e in soft {
        m.edge_mut(e).soft = true;
    }
    m
}

fn placements() -> Vec<DMat4> {
    vec![
        DMat4::from_translation(v3(3., 1., 0.)),
        DMat4::from_translation(v3(-2., 4., 0.5)) * DMat4::from_rotation_x(0.7),
        DMat4::from_translation(v3(1., -3., 0.))
            * DMat4::from_rotation_z(0.3)
            * DMat4::from_scale(v3(1.5, 1.5, 2.)),
    ]
}

type Seg = [[i64; 3]; 2];

fn key(p: DVec3) -> [i64; 3] {
    [p.x, p.y, p.z].map(|c| (c * 1e4).round() as i64)
}

fn seg(a: DVec3, b: DVec3) -> Seg {
    let (a, b) = (key(a), key(b));
    if a <= b {
        [a, b]
    } else {
        [b, a]
    }
}

/// The soft edges the rule draws from `eye`.
fn expected(eye: DVec3) -> BTreeSet<Seg> {
    let mut out = BTreeSet::new();
    for m in placements() {
        let w = |p: DVec3| m.transform_point3(p);
        // side face k spans corners k and k + 1, outward normal
        let plane = |k: usize| {
            let (p0, p1, p2) = (w(corner(k, 0.)), w(corner(k + 1, 0.)), w(corner(k + 1, 1.)));
            ((p1 - p0).cross(p2 - p0), p0)
        };
        for i in 0..SIDES {
            let (n0, c0) = plane(i);
            let (n1, c1) = plane(i + SIDES - 1);
            if (n0.dot(c0 - eye) < 0.0) != (n1.dot(c1 - eye) < 0.0) {
                out.insert(seg(w(corner(i, 0.)), w(corner(i, 1.))));
            }
        }
    }
    out
}

/// The profile lines of the viewport's next frame: its own lines in the
/// edge colour.
fn drawn(vp: &mut xtrude_app::viewport::Viewport) -> BTreeSet<Seg> {
    let ec = vp.scene.display_style.edge_color.map(|c| c as f32);
    let f = vp.frame_data([800, 600], false);
    let at = |p: [f32; 3]| f.origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
    f.lines
        .chunks_exact(2)
        .filter(|l| l.iter().all(|v| v.color[..3] == ec))
        .map(|l| seg(at(l[0].pos), at(l[1].pos)))
        .collect()
}

#[test]
fn test_profile_lines_follow_the_eye() {
    let mut vp = common::sized_viewport(800., 600.);
    vp.scene.guides.clear();
    let proto = add_mesh(&mut vp.scene.doc.meshes, cylinder());
    for m in placements() {
        let mut g = Group::new(proto, None);
        g.xform = Some(m);
        vp.scene.doc.groups.push(g);
    }
    vp.scene.version += 1;
    vp.scene.display_style.profiles = true;
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-4., -5., -1.), v3(5., 6., 3.), 1.2);
    let mut seen = BTreeSet::new();
    for (k, perspective) in [true, false].into_iter().enumerate() {
        vp.camera.perspective = perspective;
        for step in 0..6 {
            vp.camera
                .orbit(37.0 + k as f64 * 11.0, 13.0 * step as f64 - 30.0, 600);
            let eye = vp.camera.eye();
            let want = expected(eye);
            assert!(!want.is_empty(), "eye {eye}");
            assert_eq!(drawn(&mut vp), want, "eye {eye}");
            // the same eye draws the same lines
            assert_eq!(drawn(&mut vp), want, "eye {eye}, again");
            seen.extend(want);
        }
    }
    // the eyes saw the cylinders from different sides
    assert!(seen.len() > 2 * placements().len() * 2, "{}", seen.len());
    // off: none
    vp.scene.display_style.profiles = false;
    assert!(drawn(&mut vp).is_empty());
}

#[test]
fn test_profile_lines_follow_an_edit() {
    let mut vp = common::sized_viewport(800., 600.);
    vp.scene.guides.clear();
    vp.scene.display_style.profiles = true;
    let proto = add_mesh(&mut vp.scene.doc.meshes, cylinder());
    let mut g = Group::new(proto, None);
    g.xform = Some(placements()[0]);
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-4., -5., -1.), v3(5., 6., 3.), 1.2);
    let eye = vp.camera.eye();
    let one = drawn(&mut vp);
    assert_eq!(one.len(), 2, "{one:?}");
    // a second placement, same eye: its profiles join the first ones
    let mut g = Group::new(proto, None);
    g.xform = Some(placements()[1]);
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    let two = drawn(&mut vp);
    assert!(one.is_subset(&two) && two.len() == 4, "{two:?}");
    assert_eq!(vp.camera.eye(), eye);
}
