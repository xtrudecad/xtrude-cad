// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hidden Lines (K) draws the edges a face hides dashed over the
//! opaque model. Renders a box in front of a horizontal line and a diagonal
//! one, headless through wgpu, and reads the pixels; the pixel tests skip
//! where the machine offers no adapter.

use crate::common;

use common::{add_box, rgb_at, v3, window, Gpu, Pixels};
use glam::DVec3;
use serde_json::json;
use xtrude_app::window::{KeySeq, MainWindow};
use xtrude_model::style::Style;

/// The window: the wall in front, a bar and a diagonal behind it,
/// hidden-line style, seen from the front in parallel projection.
fn win() -> Option<(MainWindow, Gpu)> {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return None;
    };
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize_gl(800.0, 600.0);
    vp.scene.guides.clear();
    add_box(vp, v3(-1., -1., 0.), v3(1., 0., 2.)); // the wall, in front
    let m = vp.scene.mesh_mut();
    m.add_edge(v3(-3., 1., 1.), v3(3., 1., 1.)).unwrap(); // a bar behind it
    m.add_edge(v3(-0.8, 1., 0.2), v3(0.8, 1., 1.8)).unwrap(); // and a diagonal one
    vp.scene.version += 1;
    let st = &mut vp.scene.display_style;
    st.face_mode = "hidden_line".into();
    st.sky = false;
    st.profiles = false;
    vp.camera.set_view("front"); // looking toward +Y
    vp.camera.perspective = false;
    vp.camera.fit_to(v3(-3.2, -1., 0.), v3(3.2, 1., 2.), 1.3);
    Some((w, gpu))
}

/// Ink on paper: clearly darker than the white faces and the grey
/// background (HSL lightness).
fn dark(c: [i64; 3]) -> bool {
    lightness(c) < 0.72
}

fn lightness(c: [i64; 3]) -> f64 {
    let (hi, lo) = (c.iter().max().unwrap(), c.iter().min().unwrap());
    (hi + lo) as f64 / 2.0 / 255.0
}

/// The darkest pixel ACROSS the line at each point (±2 px along the screen
/// normal of the run of `pts`): antialiasing spreads a 1 px line over two
/// rows, and searching along it would bridge the gaps.
fn row(w: &mut MainWindow, gpu: &mut Gpu, pts: &[DVec3]) -> Vec<[i64; 3]> {
    let img: Pixels = gpu.grab(&mut w.viewport);
    let vp = &w.viewport;
    let dpr = img.0 as f64 / vp.width().max(1.0);
    let a = vp.world_to_pixel(pts[0]).unwrap();
    let b = vp.world_to_pixel(pts[pts.len() - 1]).unwrap();
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let ln = dx.hypot(dy);
    let ln = if ln == 0.0 { 1.0 } else { ln };
    let (nx, ny) = (-dy / ln, dx / ln);
    pts.iter()
        .map(|&p| {
            let px = vp.world_to_pixel(p).unwrap();
            let (x, y) = (px.x * dpr, px.y * dpr);
            // floor, not round: a line at y = 339.5 must not be skipped
            (-2..=2)
                .map(|k| {
                    let k = k as f64;
                    rgb_at(
                        &img,
                        (x + k * nx).floor() as i64,
                        (y + k * ny).floor() as i64,
                    )
                })
                .min_by(|c, d| lightness(*c).total_cmp(&lightness(*d)))
                .unwrap()
        })
        .collect()
}

fn samples(x0: f64, x1: f64) -> Vec<DVec3> {
    let n = 60;
    (0..n)
        .map(|i| v3(x0 + (x1 - x0) * i as f64 / (n - 1) as f64, 1.0, 1.0))
        .collect()
}

fn changes(v: &[bool]) -> usize {
    v.windows(2).filter(|p| p[0] != p[1]).count()
}

fn share(v: &[bool]) -> f64 {
    v.iter().filter(|b| **b).count() as f64 / v.len() as f64
}

#[test]
fn test_hidden_edges_stay_hidden_by_default() {
    let Some((mut w, mut gpu)) = win() else {
        return;
    };
    assert!(!w.viewport.scene.display_style.back_edges);
    let behind = row(&mut w, &mut gpu, &samples(-0.7, 0.7));
    assert!(!behind.iter().any(|c| dark(*c)), "{behind:?}");
    // the part of the bar that no face hides is drawn solid
    let outside = row(&mut w, &mut gpu, &samples(1.6, 2.8));
    assert!(outside.iter().all(|c| dark(*c)), "{outside:?}");
}

#[test]
fn test_hidden_lines_draws_them_dashed() {
    let Some((mut w, mut gpu)) = win() else {
        return;
    };
    assert!(w.trigger("style_hidden_lines")); // K
    assert!(w.viewport.scene.display_style.back_edges);
    let behind: Vec<bool> = row(&mut w, &mut gpu, &samples(-0.7, 0.7))
        .into_iter()
        .map(dark)
        .collect();
    assert!(0.25 < share(&behind) && share(&behind) < 0.75, "{behind:?}"); // dashes, not solid
    assert!(changes(&behind) >= 6, "{behind:?}");
    // the diagonal too: a screen-space pattern would be all dash or all gap
    let diag_pts: Vec<DVec3> = (0..60)
        .map(|i| {
            let t = i as f64 / 59.0;
            v3(-0.6 + 1.2 * t, 1.0, 0.4 + 1.2 * t)
        })
        .collect();
    let diag: Vec<bool> = row(&mut w, &mut gpu, &diag_pts)
        .into_iter()
        .map(dark)
        .collect();
    // (without antialiasing a 1 px diagonal is a staircase that sampling
    // across it partly misses: neither solid nor empty is what matters)
    assert!(0.12 < share(&diag) && share(&diag) < 0.75, "{diag:?}");
    assert!(changes(&diag) >= 6, "{diag:?}");
    // the visible part of the bar is still solid
    let outside = row(&mut w, &mut gpu, &samples(1.6, 2.8));
    assert!(outside.iter().all(|c| dark(*c)), "{outside:?}");
}

#[test]
fn test_k_is_hidden_lines_and_the_style_remembers_it() {
    let s = Style {
        back_edges: true,
        ..Style::default()
    };
    assert!(Style::from_json(&s.to_json()).back_edges);
    assert!(!Style::from_json(&json!({"name": "Old"})).back_edges);
    let w = window();
    let k = KeySeq::parse("K").unwrap();
    let holders: Vec<String> = w.actions.owners(k).iter().map(|a| a.id.clone()).collect();
    assert_eq!(holders, vec!["style_hidden_lines".to_string()]);
}
