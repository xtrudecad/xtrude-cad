// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A painted group puts its colour in the Colors panel's «In model» list.

use crate::common;

use common::{v3, window};
use xtrude_app::tray::colors::Colors;
use xtrude_core::attrs::Paint;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};

#[test]
fn test_a_painted_group_puts_its_colour_in_the_model_list() {
    let mut w = window();
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some("Box"));
    g.material = Some(Paint {
        color: Some([0.8, 0.2, 0.2]),
        mat: Some("Brick red".into()),
        opacity: None,
    });
    w.viewport.scene.doc.groups.push(g);
    let mut p = Colors::default();
    p.refresh_in_model(&w);
    assert!(p.in_model.iter().any(|s| s.tip.contains("Brick red")));
}
