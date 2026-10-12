// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Guide crossings are exact, the snap working on each guide's span clipped to
//! the view.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{window, MetreDocument};
use glam::DVec3;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::topology::{segment_intersection, SPLIT_TOLERANCE};
use xtrude_model::guide::Guide;

fn exact_crossing(g1: &Guide, g2: &Guide) -> (f64, f64) {
    let (p, d) = (g1.point, g1.direction.unwrap());
    let (q, e) = (g2.point, g2.direction.unwrap());
    let den = d.x * e.y - d.y * e.x;
    let s = ((q.x - p.x) * e.y - (q.y - p.y) * e.x) / den;
    (p.x + d.x * s, p.y + d.y * s)
}

#[test]
fn test_guide_crossings_are_exact_in_top_view() {
    let _m = MetreDocument::new();
    let mut w = window();
    w.viewport.resize(1200., 800.);
    w.viewport.camera.set_aspect(1200, 800);
    let vp = &w.viewport;
    let mut rnd = Mt19937::new(110);
    let (mut worst, mut checked) = (0.0f64, 0);
    for _ in 0..200 {
        let a = rnd.uniform(10., 80.).to_radians();
        let choices = [0.0, 90.0, rnd.uniform(0., 180.)];
        let b = choices[rnd.choice_index(3)].to_radians();
        if (a - b).sin().abs() < 0.1 {
            continue;
        }
        let g1 = Guide::new(
            DVec3::new(rnd.uniform(-3., 3.), rnd.uniform(-3., 3.), 0.),
            Some(DVec3::new(a.cos(), a.sin(), 0.)),
            None,
        );
        let g2 = Guide::new(
            DVec3::new(rnd.uniform(-3., 3.), rnd.uniform(-3., 3.), 0.),
            Some(DVec3::new(b.cos(), b.sin(), 0.)),
            None,
        );
        let (x, y) = exact_crossing(&g1, &g2);
        let (Some(s1), Some(s2)) = (
            vp.guide_snap_segment(&g1, 1.5),
            vp.guide_snap_segment(&g2, 1.5),
        ) else {
            continue;
        };
        let Some(hit) = segment_intersection(s1.0, s1.1, s2.0, s2.1, SPLIT_TOLERANCE) else {
            continue; // the crossing is outside the view
        };
        checked += 1;
        worst = worst.max((hit.x - x).hypot(hit.y - y));
    }
    assert!(checked > 50, "{checked}");
    assert!(worst < 1e-5, "{worst}"); // below 0.01 mm
}

#[test]
fn test_clipped_guide_stays_on_its_line_and_off_screen() {
    let _m = MetreDocument::new();
    let mut w = window();
    w.viewport.resize(1200., 800.);
    w.viewport.camera.set_aspect(1200, 800);
    let vp = &w.viewport;
    let g = Guide::new(
        DVec3::new(0.3, -0.2, 0.),
        Some(DVec3::new(1., 1., 0.)),
        None,
    );
    let d = g.direction.unwrap();
    let (a, b) = vp.guide_snap_segment(&g, 1.5).expect("a span");
    for p in [a, b] {
        let r = p - g.point;
        assert!((r.x * d.y - r.y * d.x).abs() < 1e-5); // on the line…
                                                       // …and its ends off the viewport, so they never read as endpoints.
        let px = vp.world_to_pixel(p);
        assert!(px.is_none_or(
            |ScreenPt { x, y }| !((0.0..=1200.).contains(&x) && (0.0..=800.).contains(&y))
        ));
    }
}
