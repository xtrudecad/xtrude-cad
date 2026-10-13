// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Faces with holes through the document.

use crate::common;
use common::*;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

#[test]
fn test_document_roundtrip_preserves_holes() {
    let mut s = Scene::new();
    let mut h = History::new();
    let sq = vec![
        v3(0., 0., 0.),
        v3(4., 0., 0.),
        v3(4., 4., 0.),
        v3(0., 4., 0.),
    ];
    let hole = vec![
        v3(1., 1., 0.),
        v3(3., 1., 0.),
        v3(3., 3., 0.),
        v3(1., 3., 0.),
    ];
    h.execute(&mut s, &mut AddFaceCommand::new(sq));
    h.execute(&mut s, &mut AddFaceCommand::new(hole));
    let loaded = roundtrip(&s);
    let m = loaded.mesh();
    let mother = m.face_ids()[0];
    assert_eq!(m.face(mother).holes.len(), 1);
    let area: f64 = m
        .face_triangulate(mother, None)
        .iter()
        .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
        .sum();
    assert!((area - 12.0).abs() < 1e-6);
}
