// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STL import timing on a 100k-triangle binary file (a UV sphere).

use std::io::Write;

use criterion::{criterion_group, criterion_main, Criterion};
use glam::DVec3;
use xtrude_io::stl::{load_stl, SimplifyMode};
use xtrude_model::Scene;

/// A UV sphere of radius 50 mm with `rings × segments × 2` triangles.
fn sphere_stl(path: &std::path::Path, rings: usize, segments: usize) -> std::io::Result<usize> {
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
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    out.write_all(&[0u8; 80])?;
    out.write_all(&(tris.len() as u32).to_le_bytes())?;
    for t in &tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        for v in std::iter::once(n).chain(t.iter().copied()) {
            for x in [v.x, v.y, v.z] {
                out.write_all(&(x as f32).to_le_bytes())?;
            }
        }
        out.write_all(&[0u8; 2])?;
    }
    Ok(tris.len())
}

fn bench_stl(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    let path = std::env::temp_dir().join(format!("xtrude-bench-{}.stl", std::process::id()));
    let n = sphere_stl(&path, 225, 224).expect("write the sphere");
    assert!(n >= 100_000, "{n} triangles");
    let mut g = c.benchmark_group("stl_import");
    g.sample_size(10);
    g.bench_function("sphere_100k", |b| {
        b.iter(|| {
            let mut s = Scene::new();
            load_stl(&mut s, &path, None, 0.001, SimplifyMode::None).expect("load");
        })
    });
    g.finish();
    let _ = std::fs::remove_file(&path);
}

criterion_group!(benches, bench_stl);
criterion_main!(benches);
