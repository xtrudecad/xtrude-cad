// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Frame building on a 100k-triangle STL: an orbit frame (cached scene
//! buffers) and a frame after an edit (scene buffers rebuilt); and an
//! orbit frame and a still one of 1000 component placements (192k placed
//! hard edges and 96k soft ones).

use criterion::{criterion_group, criterion_main, Criterion};
use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_io::stl::{load_stl, SimplifyMode};
use xtrude_model::group::{add_mesh, Group};

fn sphere(path: &std::path::Path, rings: usize, segments: usize) {
    use std::io::Write;
    let at = |r: usize, s: usize| {
        let th = std::f64::consts::PI * r as f64 / rings as f64;
        let ph = std::f64::consts::TAU * s as f64 / segments as f64;
        DVec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()) * 50.0
    };
    let mut tris = Vec::new();
    for r in 0..rings {
        for s in 0..segments {
            let (a, b, c, d) = (at(r, s), at(r + 1, s), at(r + 1, s + 1), at(r, s + 1));
            if r > 0 {
                tris.push([a, b, d]);
            }
            if r + 1 < rings {
                tris.push([b, c, d]);
            }
        }
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    out.write_all(&[0u8; 80]).unwrap();
    out.write_all(&(tris.len() as u32).to_le_bytes()).unwrap();
    for t in &tris {
        for v in std::iter::once(DVec3::ZERO).chain(t.iter().copied()) {
            for x in [v.x, v.y, v.z] {
                out.write_all(&(x as f32).to_le_bytes()).unwrap();
            }
        }
        out.write_all(&[0u8; 2]).unwrap();
    }
}

fn bench_frame(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    let path = std::env::temp_dir().join(format!("xtrude-frame-{}.stl", std::process::id()));
    sphere(&path, 225, 224);
    let mut vp = Viewport::new();
    vp.resize(1280., 800.);
    vp.camera.set_aspect(1280, 800);
    load_stl(&mut vp.scene, &path, None, 0.001, SimplifyMode::None).expect("load");
    let _ = std::fs::remove_file(&path);
    vp.camera.set_view("iso");
    vp.camera
        .fit_to(DVec3::splat(-0.06), DVec3::splat(0.06), 1.2);
    vp.frame_data([1280, 800], false);
    let mut g = c.benchmark_group("frame_100k");
    g.sample_size(10);
    g.bench_function("orbit_frame", |b| {
        b.iter(|| {
            vp.camera.orbit(2.0, 0.0, 800);
            vp.frame_data([1280, 800], false)
        })
    });
    g.bench_function("frame_after_edit", |b| {
        b.iter(|| {
            vp.scene.version += 1;
            vp.frame_data([1280, 800], false)
        })
    });
    g.finish();
}

/// A closed cylinder of `sides` sides with soft sides: `2 * sides` hard
/// edges, `sides` soft ones.
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
    let sides: Vec<_> = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            (a.z - b.z).abs() > 0.5
        })
        .collect();
    for e in sides {
        m.edge_mut(e).soft = true;
    }
    m
}

fn bench_frame_placements(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    let mut vp = Viewport::new();
    vp.resize(1280., 800.);
    vp.camera.set_aspect(1280, 800);
    let proto = add_mesh(&mut vp.scene.doc.meshes, cylinder(96));
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
    vp.camera.set_view("iso");
    vp.camera.fit_to(DVec3::ZERO, DVec3::new(80., 50., 1.), 1.1);
    vp.frame_data([1280, 800], false);
    let mut g = c.benchmark_group("frame_1000_placements");
    g.sample_size(10);
    g.bench_function("orbit_frame", |b| {
        b.iter(|| {
            vp.camera.orbit(2.0, 0.0, 800);
            vp.frame_data([1280, 800], false)
        })
    });
    // the eye still (a hover repaint): the profiles of the last frame
    g.bench_function("still_frame", |b| {
        b.iter(|| vp.frame_data([1280, 800], false))
    });
    g.finish();
}

criterion_group!(benches, bench_frame, bench_frame_placements);
criterion_main!(benches);
