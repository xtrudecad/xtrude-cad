// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Face sides through the document.

use crate::common;
use common::*;
use xtrude_core::attrs::{Back, Paint};
use xtrude_model::scene::Scene;

#[test]
fn test_document_saves_the_two_sided_face_as_true() {
    let mut s = Scene::new();
    let m = s.mesh_mut();
    let dos = quad(m, 0., 4., 0., 4., 0.);
    m.face_mut(dos).attrs.paint.color = Some([1., 0., 0.]);
    m.face_mut(dos).attrs.back = Some(Back::Mirror);
    let own = quad(m, 6., 8., 0., 2., 0.);
    m.face_mut(own).attrs.paint.color = Some([1., 0., 0.]);
    m.face_mut(own).attrs.back = Some(Back::Own(Paint {
        color: Some([0., 0., 1.]),
        ..Default::default()
    }));
    let solo = quad(m, 10., 12., 0., 2., 0.);
    m.face_mut(solo).attrs.paint.color = Some([1., 0., 0.]);
    let back = roundtrip(&s);
    let m = back.mesh();
    let by_x = |x: f64| {
        m.face_ids()
            .into_iter()
            .find(|&f| {
                (m.face_positions(f)
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::MAX, f64::min)
                    - x)
                    .abs()
                    < 0.5
            })
            .unwrap()
    };
    assert_eq!(m.face(by_x(0.)).attrs.back, Some(Back::Mirror));
    assert_eq!(
        m.face(by_x(6.)).attrs.back,
        Some(Back::Own(Paint {
            color: Some([0., 0., 1.]),
            ..Default::default()
        }))
    );
    assert_eq!(m.face(by_x(10.)).attrs.back, None);
}
