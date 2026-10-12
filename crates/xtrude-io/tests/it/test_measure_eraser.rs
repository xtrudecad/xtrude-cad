// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Guides through .igz.

use crate::common;
use common::*;
use xtrude_model::guide::Guide;
use xtrude_model::scene::Scene;

#[test]
fn test_guides_igz_round_trip() {
    let mut s = Scene::new();
    s.doc
        .guides
        .push(Guide::new(v3(1., 2., 0.), Some(v3(0., 1., 0.)), None));
    s.doc.guides.push(Guide::new(v3(5., 5., 0.), None, None));
    let loaded = roundtrip(&s);
    assert_eq!(loaded.doc.guides.len(), 2);
    assert!(loaded.doc.guides[0].is_line() && !loaded.doc.guides[1].is_line());
}
