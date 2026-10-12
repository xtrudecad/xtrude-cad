// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The dash generator half: guides are world-space dashes so the
//! depth buffer hides the ones behind a wall. The two pixel tests draw the
//! viewport's frame, so they are in xtrude-app's test file of the same name.

use glam::DVec3;
use xtrude_model::guide::Guide;
use xtrude_render::lines::guide_vertices;

#[test]
fn test_the_generator_lays_dashes_along_the_guide() {
    let g = Guide::new(DVec3::new(0., 0., 1.), Some(DVec3::X), None); // +X at z = 1
    let (pts, spans) = guide_vertices(&[g], 1.0, &[], None, 0.0);
    assert_eq!(spans.len(), 1);
    let (start, count, selected) = spans[0];
    assert!(start == 0 && count > 2 && count % 2 == 0); // pairs
    assert!(!selected);
    assert!(pts.iter().all(|p| p.y.abs() < 1e-12)); // never leaves the guide…
    assert!(pts.iter().all(|p| (p.z - 1.0).abs() < 1e-12)); // …either way
    assert!(pts[1].x < pts[2].x - 1e-9); // gaps: dashes do not touch
}

#[test]
fn test_a_guide_point_produces_no_dashes() {
    let (pts, spans) = guide_vertices(
        &[Guide::new(DVec3::new(1., 2., 3.), None, None)],
        1.0,
        &[],
        None,
        0.0,
    );
    assert!(spans.is_empty() && pts.is_empty());
}

#[test]
fn test_the_selected_guide_is_flagged_for_its_own_colour() {
    let g1 = Guide::new(DVec3::ZERO, Some(DVec3::X), None);
    let g2 = Guide::new(DVec3::ZERO, Some(DVec3::Y), None);
    let sel = [g2.id];
    let (_pts, spans) = guide_vertices(&[g1, g2], 1.0, &sel, None, 0.0);
    assert_eq!(
        spans.iter().map(|s| s.2).collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn test_the_dash_count_is_capped_however_long_the_guide() {
    let g = Guide::new(DVec3::ZERO, Some(DVec3::X), None);
    let (_pts, spans) = guide_vertices(&[g], 0.001, &[], None, 0.0);
    assert!(spans[0].1 <= 8000); // 4000 dashes × 2 vertices
}
