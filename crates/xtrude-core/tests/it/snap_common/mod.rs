// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Helpers shared by the snap-engine tests.

pub use xtrude_core::snap::{compute_snap, SnapEdge, SnapKind, SnapQuery, SnapResult};
pub use xtrude_core::DVec3;

pub fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// The tests' plan-view camera: 100 px per metre, x right, y down the page.
pub fn w2p(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, p.y * 100.0))
}

pub fn px(p: DVec3) -> (f64, f64) {
    (p.x * 100.0, p.y * 100.0)
}

/// `compute_snap(cand, w2p(cand), scene, w2p, threshold_px=9, edge_threshold_px=14)`
pub fn query(cand: DVec3) -> SnapQuery<'static> {
    let mut q = SnapQuery::new(cand, px(cand), &w2p, 9.0);
    q.edge_threshold_px = Some(14.0);
    q
}

pub fn edge(a: DVec3, b: DVec3) -> SnapEdge {
    SnapEdge::segment(a, b)
}

/// A guide as the snap scene feeds it: a ±10 km segment.
pub fn guide(point: DVec3, direction: DVec3) -> SnapEdge {
    let d = direction.normalize();
    edge(point - d * 1.0e4, point + d * 1.0e4)
}

/// The cursor ray of a plan view: the closest point of the line to `cand`.
pub fn proj_factory(cand: DVec3) -> impl Fn(DVec3, DVec3) -> DVec3 {
    move |start: DVec3, direction: DVec3| {
        let d = direction.normalize();
        start + d * (cand - start).dot(d)
    }
}

/// A query with the tests' usual radii and a custom projection.
pub fn query_with<'a>(cand: DVec3, w2p: &'a dyn Fn(DVec3) -> Option<(f64, f64)>) -> SnapQuery<'a> {
    let px = w2p(cand).unwrap_or((0.0, 0.0));
    let mut q = SnapQuery::new(cand, px, w2p, 9.0);
    q.edge_threshold_px = Some(14.0);
    q
}

/// `(x·100, -y·100)`: y up the screen.
pub fn w2p_up(p: DVec3) -> Option<(f64, f64)> {
    Some((p.x * 100.0, -p.y * 100.0))
}
