// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3D text letters through the document.

use crate::common;
use common::*;
use xtrude_model::scene::Scene;
use xtrude_model::text3d::{make_text_group, text_is_pristine, TextParams};

#[test]
fn test_text_group_round_trips_through_document() {
    let mut s = Scene::new();
    let g = make_text_group(&mut s.doc.meshes, &TextParams::new("Bracket", "Sans")).unwrap();
    s.doc.groups.push(g);
    let back = roundtrip(&s);
    let g2 = &back.doc.groups[0];
    assert_eq!(g2.text3d, s.doc.groups[0].text3d);
    assert_eq!(
        g2.children
            .iter()
            .map(|k| k.name.as_str())
            .collect::<String>(),
        "Bracket"
    );
}

#[test]
fn test_a_fresh_text_is_pristine_and_stays_so_through_document() {
    let mut s = Scene::new();
    let g = make_text_group(&mut s.doc.meshes, &TextParams::new("Bracket", "Sans")).unwrap();
    assert!(g.text3d.as_ref().unwrap()["hash"]
        .as_str()
        .is_some_and(|h| !h.is_empty()));
    assert!(text_is_pristine(&s.doc.meshes, &g));
    s.doc.groups.push(g);
    let back = roundtrip(&s);
    assert!(text_is_pristine(&back.doc.meshes, &back.doc.groups[0]));
}
