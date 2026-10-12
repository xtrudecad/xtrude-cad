// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `hull_2d` is the monotone chain itself (no speed prefilter to get wrong),
//! checked here against an independent one on the same awkward sets; and a
//! container's box follows a moved child and an edited prototype (`group_obb`
//! reads the points each time).

use crate::common;

use common::{stub, v3};
use glam::DMat4;
use std::f64::consts::PI;
use std::sync::Arc;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, hull_2d, Group};

/// The plain monotone chain, written out again.
fn plain_hull(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    p.dedup();
    let half = |seq: Vec<(f64, f64)>| {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for q in seq {
            while out.len() >= 2 {
                let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
                if (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0) > 0.0 {
                    break;
                }
                out.pop();
            }
            out.push(q);
        }
        out
    };
    let lower = half(p.clone());
    let upper = half(p.into_iter().rev().collect());
    let mut out = lower[..lower.len() - 1].to_vec();
    out.extend_from_slice(&upper[..upper.len() - 1]);
    out
}

fn same_polygon(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let close = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9;
    (0..b.len()).any(|k| (0..a.len()).all(|i| close(a[i], b[(k + i) % b.len()])))
}

#[test]
fn test_hull_matches_the_plain_monotone_chain_on_awkward_sets() {
    let mut rng = Mt19937::new(7);
    let mut u = |a: f64, b: f64| rng.uniform(a, b);
    let cloud: Vec<(f64, f64)> = (0..5000).map(|_| (u(-50., 50.), u(-50., 50.))).collect();
    let mut dups: Vec<(f64, f64)> = (0..4000).map(|_| (u(0., 100.), u(0., 100.))).collect();
    for _ in 0..50 {
        let p = (u(0., 100.), u(0., 100.));
        dups.extend(std::iter::repeat_n(p, 20));
    }
    // A rectangular site with slightly bowed edges (the plaza).
    let mut site = Vec::new();
    for i in 0..3000 {
        let t = i as f64 / 2999.0;
        let bow = 0.3 * (PI * t).sin();
        site.push((t * 120., 80. + bow));
        site.push((t * 120., -bow));
        site.push((-bow, t * 80.));
        site.push((120. + bow, t * 80.));
    }
    site.extend((0..3000).map(|_| (u(0., 100.), u(0., 100.))));
    for pts in [cloud, dups, site] {
        assert!(same_polygon(&hull_2d(&pts), &plain_hull(&pts)));
    }
}

#[test]
fn test_the_containers_box_reuses_the_prototypes_point_arrays() {
    let mut vp = stub();
    let mut proto = Mesh::new();
    proto.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let pid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let kids: Vec<Group> = (0..3)
        .map(|i| {
            let mut g = Group::new(pid, Some(&format!("copy {i}")));
            g.xform = Some(DMat4::from_translation(v3(3. * i as f64, 0., 0.)));
            g
        })
        .collect();
    let mut cont = Group::new(
        add_mesh(&mut vp.scene.doc.meshes, Mesh::new()),
        Some("container"),
    );
    cont.xform = Some(DMat4::IDENTITY);
    cont.children = kids;
    let cid = cont.id;
    vp.scene.doc.groups.push(cont);
    vp.scene.version += 1;
    let (_f, lo, hi) = vp.group_obb(cid).unwrap();
    assert!(hi.y - lo.y < 1.5);
    // A child moved: a new box.
    vp.scene.doc.groups[0].children[1].xform = Some(DMat4::from_translation(v3(3., 5., 0.)));
    vp.scene.version += 1;
    let (_f, lo, hi) = vp.group_obb(cid).unwrap();
    assert!(hi.y - lo.y > 5.5);
    // The prototype itself edited: the box follows (x from 0..7 to −2..7).
    let mut m = (*vp.scene.doc.meshes[&pid]).clone();
    let v0 = m
        .vertex_ids()
        .into_iter()
        .find(|&v| m.pos(v).length() < 1e-9)
        .unwrap();
    m.move_vertex(v0, v3(-2., 0., 0.));
    vp.scene.doc.meshes.insert(pid, Arc::new(m));
    vp.scene.version += 1;
    let (_f, lo, hi) = vp.group_obb(cid).unwrap();
    let d = hi - lo;
    assert!(d.x.max(d.y).max(d.z) > 8.5, "{lo} {hi}");
}
