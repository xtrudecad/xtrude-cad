// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hidden edges through the document.

use crate::common;
use common::*;
use xtrude_core::DVec3;
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

#[test]
fn test_hidden_round_trips_through_document() {
    let mut s = Scene::new();
    let mut h = History::new();
    let sq = [
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 2., 0.),
        v3(0., 2., 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect();
    let mut cmd = build_add_edges(&s, &segs, true, vec![], None);
    h.execute(&mut s, &mut *cmd);
    let e = s.mesh().edge_ids()[2];
    s.mesh_mut().edge_mut(e).hidden = true;
    let loaded = roundtrip(&s);
    let m = loaded.mesh();
    assert_eq!(
        m.edge_ids().iter().filter(|&&e| m.edge(e).hidden).count(),
        1
    );
}
