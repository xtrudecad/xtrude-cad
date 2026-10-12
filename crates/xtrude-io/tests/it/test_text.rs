// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Text through .igz.

use crate::common;
use common::*;
use xtrude_model::scene::Scene;
use xtrude_model::textlabel::TextLabel;

#[test]
fn test_label_igz_round_trip() {
    let mut s = Scene::new();
    s.doc.text_labels.push(TextLabel::new(
        v3(1., 2., 3.),
        v3(0., 0., 1.),
        "Tank\n10 m³",
    ));
    let mut s2 = roundtrip(&s);
    assert_eq!(s2.doc.text_labels.len(), 1);
    let lab = &s2.doc.text_labels[0];
    assert_eq!(lab.text, "Tank\n10 m³");
    assert!((lab.position() - v3(1., 2., 4.)).length() < 1e-6);
    s2.clear();
    assert!(s2.doc.text_labels.is_empty());
}
