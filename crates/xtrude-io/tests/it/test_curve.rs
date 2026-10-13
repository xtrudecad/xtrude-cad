// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Curves through the document.

use crate::common;
use common::*;
use xtrude_core::DVec3;
use xtrude_io::document;
use xtrude_model::commands::{AddFaceCommand, TagCurveCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::{Command, History};
use xtrude_model::scene::Scene;

#[test]
fn test_curve_id_survives_document() {
    let mut s = Scene::new();
    let mut h = History::new();
    let pts: Vec<DVec3> = (0..24)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / 24.0;
            v3(5.0 * a.cos(), 5.0 * a.sin(), 0.0)
        })
        .collect();
    let segs: Vec<(DVec3, DVec3)> = (0..24).map(|i| (pts[i], pts[(i + 1) % 24])).collect();
    let extra: Vec<Box<dyn Command>> = vec![
        Box::new(AddFaceCommand::new(pts.clone())),
        Box::new(TagCurveCommand {
            pts: pts.clone(),
            closed: true,
        }),
    ];
    let mut cmd = build_add_edges(&s, &segs, false, extra, None);
    h.execute(&mut s, &mut *cmd);
    let tmp = TmpDir::new();
    let path = tmp.join("circle.xtrude");
    document::save_scene(&s, &path).unwrap();
    let mut loaded = Scene::new();
    document::load_into(&mut loaded, &path, None).unwrap();
    let m = loaded.mesh();
    let ids: std::collections::HashSet<Option<i64>> =
        m.edge_ids().iter().map(|&e| m.edge(e).curve).collect();
    assert!(ids.len() == 1 && !ids.contains(&None));
    assert_eq!(m.curve_edges(m.edge_ids()[0]).len(), 24);
}
