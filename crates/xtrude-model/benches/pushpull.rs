// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Push/pull timing: a fresh prism, and a wall of a solid whose top is cut
//! into many faces.

use criterion::{criterion_group, criterion_main, Criterion};
use xtrude_core::DVec3;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;
use xtrude_model::{History, Scene};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> [DVec3; 4] {
    [
        DVec3::new(x0, y0, 0.),
        DVec3::new(x1, y0, 0.),
        DVec3::new(x1, y1, 0.),
        DVec3::new(x0, y1, 0.),
    ]
}

fn bench_pushpull(c: &mut Criterion) {
    xtrude_model::settings::use_memory_store();
    c.bench_function("pushpull_rectangle", |b| {
        b.iter(|| {
            let (mut s, mut h) = (Scene::new(), History::new());
            let f = s.mesh_mut().add_face(&rect(0., 0., 4., 3.), &[]);
            assert!(push_face(&mut s, &mut h, MeshId::LOOSE, f, 2.5, false));
        })
    });
    // A 12 × 12 grid of tiles pushed into blocks, then one more tile pushed.
    let busy = || {
        let (mut s, mut h) = (Scene::new(), History::new());
        let mut tiles = Vec::new();
        for i in 0..12 {
            for j in 0..12 {
                let (x, y) = (i as f64 * 1.5, j as f64 * 1.5);
                tiles.push(s.mesh_mut().add_face(&rect(x, y, x + 1., y + 1.), &[]));
            }
        }
        for &f in &tiles[1..] {
            push_face(&mut s, &mut h, MeshId::LOOSE, f, 1.0, false);
        }
        (s, h, tiles[0])
    };
    let mut g = c.benchmark_group("pushpull_busy");
    g.sample_size(20);
    g.bench_function("pushpull_in_busy_mesh", |b| {
        b.iter_batched(
            busy,
            |(mut s, mut h, f)| push_face(&mut s, &mut h, MeshId::LOOSE, f, 1.0, false),
            criterion::BatchSize::LargeInput,
        )
    });
    g.finish();
}

criterion_group!(benches, bench_pushpull);
criterion_main!(benches);
