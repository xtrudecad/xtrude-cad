// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Picking timing: a face under the cursor among a field of boxes, and the
//! Select tool's hover over a large model of component placements.

use criterion::{criterion_group, criterion_main, Criterion};
use glam::{DMat4, DVec3};
use xtrude_app::input::MouseEvent;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

fn add_box(vp: &mut Viewport, lo: DVec3, hi: DVec3) {
    let c = |x: bool, y: bool, z: bool| {
        DVec3::new(
            if x { hi.x } else { lo.x },
            if y { hi.y } else { lo.y },
            if z { hi.z } else { lo.z },
        )
    };
    let faces = [
        [
            c(false, false, false),
            c(false, true, false),
            c(true, true, false),
            c(true, false, false),
        ],
        [
            c(false, false, true),
            c(true, false, true),
            c(true, true, true),
            c(false, true, true),
        ],
        [
            c(false, false, false),
            c(true, false, false),
            c(true, false, true),
            c(false, false, true),
        ],
        [
            c(false, true, false),
            c(false, true, true),
            c(true, true, true),
            c(true, true, false),
        ],
        [
            c(false, false, false),
            c(false, false, true),
            c(false, true, true),
            c(false, true, false),
        ],
        [
            c(true, false, false),
            c(true, true, false),
            c(true, true, true),
            c(true, false, true),
        ],
    ];
    for f in faces {
        vp.scene.mesh_mut().add_face(&f, &[]);
    }
}

fn bench_pick(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    let mut vp = Viewport::new();
    vp.resize(1280., 800.);
    vp.camera.set_aspect(1280, 800);
    for i in 0..40 {
        for j in 0..40 {
            let lo = DVec3::new(i as f64 * 2., j as f64 * 2., 0.);
            add_box(
                &mut vp,
                lo,
                lo + DVec3::new(1., 1., 1. + (i + j) as f64 * 0.05),
            );
        }
    }
    vp.scene.version += 1;
    vp.camera.set_view("iso");
    vp.camera.fit_to(DVec3::ZERO, DVec3::new(80., 80., 5.), 1.1);
    vp.pick_face(640., 400.);
    c.bench_function("pick_face_9600_faces", |b| {
        b.iter(|| vp.pick_face(640., 400.))
    });
    c.bench_function("pick_face_cold_index", |b| {
        b.iter(|| {
            vp.scene.version += 1;
            vp.pick_face(640., 400.)
        })
    });
}

/// A closed cylinder of `sides` sides: `3 * sides` hard edges.
fn cylinder(sides: usize) -> Mesh {
    let mut m = Mesh::new();
    let at = |i: usize, z: f64| {
        let a = std::f64::consts::TAU * i as f64 / sides as f64;
        DVec3::new(a.cos() * 0.4, a.sin() * 0.4, z)
    };
    let bottom: Vec<DVec3> = (0..sides).rev().map(|i| at(i, 0.)).collect();
    let top: Vec<DVec3> = (0..sides).map(|i| at(i, 1.)).collect();
    m.add_face(&bottom, &[]);
    m.add_face(&top, &[]);
    for i in 0..sides {
        let j = (i + 1) % sides;
        m.add_face(&[at(i, 0.), at(j, 0.), at(j, 1.), at(i, 1.)], &[]);
    }
    m
}

/// The Select tool hovering a field of 1000 placements of one 64-sided
/// cylinder component: 192k placed hard edges, 252k triangles.
fn bench_hover_large(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    let mut vp = Viewport::new();
    vp.resize(1280., 800.);
    vp.camera.set_aspect(1280, 800);
    let proto = add_mesh(&mut vp.scene.doc.meshes, cylinder(64));
    for i in 0..40 {
        for j in 0..25 {
            let mut g = Group::new(proto, None);
            g.xform = Some(DMat4::from_translation(DVec3::new(
                i as f64 * 2.,
                j as f64 * 2.,
                0.,
            )));
            vp.scene.doc.groups.push(g);
        }
    }
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(SelectTool::new())));
    vp.camera.set_view("iso");
    vp.camera.fit_to(DVec3::ZERO, DVec3::new(80., 50., 1.), 1.1);
    let face = MouseEvent::at(640., 400.);
    let background = MouseEvent::at(4., 4.);
    vp.mouse_move(&face);
    assert!(vp.pick_index().group_edge_count() >= 192_000);
    let mut g = c.benchmark_group("hover_192k_group_edges");
    g.sample_size(10);
    g.bench_function("background", |b| b.iter(|| vp.mouse_move(&background)));
    g.bench_function("face", |b| b.iter(|| vp.mouse_move(&face)));
    g.bench_function("background_after_orbit", |b| {
        b.iter(|| {
            vp.camera.orbit(0.5, 0.0, 800);
            vp.mouse_move(&background)
        })
    });
    g.finish();
}

criterion_group!(benches, bench_pick, bench_hover_large);
criterion_main!(benches);
