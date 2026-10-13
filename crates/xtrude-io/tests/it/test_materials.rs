// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Materials through the document.

use crate::common;
use common::*;
use xtrude_core::DVec3;
use xtrude_io::document;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::{Command, History};
use xtrude_model::scene::Scene;

fn square(s: &mut Scene, h: &mut History) -> xtrude_core::FaceId {
    let lp = vec![
        v3(0., 0., 0.),
        v3(4., 0., 0.),
        v3(4., 4., 0.),
        v3(0., 4., 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (lp[i], lp[(i + 1) % 4])).collect();
    let face: Box<dyn Command> = Box::new(AddFaceCommand::new(lp));
    let mut cmd = build_add_edges(s, &segs, false, vec![face], None);
    h.execute(s, &mut *cmd);
    s.mesh().face_ids()[0]
}

#[test]
fn test_color_survives_document_round_trip() {
    let mut s = Scene::new();
    let mut h = History::new();
    let f = square(&mut s, &mut h);
    s.mesh_mut().face_mut(f).attrs.paint.color = Some([0.3, 0.6, 0.9]);
    let loaded = roundtrip(&s);
    let m = loaded.mesh();
    assert_eq!(
        m.face(m.face_ids()[0]).attrs.paint.color,
        Some([0.3, 0.6, 0.9])
    );
}

#[test]
fn test_unpainted_face_writes_no_color_key() {
    let mut s = Scene::new();
    let mut h = History::new();
    square(&mut s, &mut h);
    let tmp = TmpDir::new();
    let path = tmp.join("plain.xtrude");
    document::save_scene(&s, &path).unwrap();
    let data = document_json(&path);
    assert!(data["scene"]["faces"][0].get("color").is_none());
}
