// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Parts-tray test: dragging the exploded-view slider previews live and lands
//! as one undo step. (The command tests are in xtrude-io.)

use crate::common;

use common::{v3, window};
use xtrude_app::tray::Tray;
use xtrude_app::window::MainWindow;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, world_mesh, Exploded, Group, MeshTable};
use xtrude_model::ids::GroupId;

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn cube(t: &mut MeshTable, at: (f64, f64, f64)) -> Group {
    let mut c = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                c.push(v3(at.0 + dx, at.1 + dy, at.2 + dz));
            }
        }
    }
    let mut m = Mesh::new();
    for q in QUADS {
        m.add_face(&q.map(|k| c[k]), &[]);
    }
    let mut g = Group::new(add_mesh(t, m), None);
    g.xform = Some(DMat4::IDENTITY);
    g
}

/// A centre cube, one on each side along red, one above; placed.
fn assembly(w: &mut MainWindow) -> GroupId {
    let t = &mut w.viewport.scene.doc.meshes;
    let kids = vec![
        cube(t, (0., 0., 0.)),
        cube(t, (-3., 0., 0.)),
        cube(t, (3., 0., 0.)),
        cube(t, (0., 0., 3.)),
    ];
    let mut b = Group::new(add_mesh(t, Mesh::new()), Some("box"));
    b.adopt(kids);
    b.xform = Some(DMat4::from_translation(v3(10., 5., 0.)) * rotation_deg(DVec3::Z, 35.0));
    let id = b.id;
    w.viewport.scene.doc.groups.push(b);
    id
}

fn world(w: &MainWindow, g: GroupId) -> Vec<(i64, i64, i64)> {
    let doc = &w.viewport.scene.doc;
    let wm = world_mesh(&doc.meshes, doc.find_group(g).unwrap());
    let mut v: Vec<_> = wm
        .verts
        .iter()
        .map(|(_, x)| {
            let p = x.position;
            (
                (p.x * 1e5).round() as i64,
                (p.y * 1e5).round() as i64,
                (p.z * 1e5).round() as i64,
            )
        })
        .collect();
    v.sort();
    v
}

fn exploded(w: &MainWindow, g: GroupId) -> Option<Exploded> {
    w.viewport.scene.doc.find_group(g).unwrap().exploded.clone()
}

#[test]
fn test_the_tray_slider_is_one_undo_step_per_drag() {
    let mut w = window();
    let bx = assembly(&mut w);
    let scene = &mut w.viewport.scene;
    scene.selection.clear();
    scene.selection.insert(EntityRef::Group(bx));
    scene.version += 1;
    let assembled = world(&w, bx);
    let mut tray = Tray::default();
    let panel = &mut tray.parts;
    panel.refresh(&w);
    let before = w.viewport.history.undo_len();
    panel.explode_press(&mut w);
    for v in [10, 40, 90, 120] {
        panel.explode_value(&mut w, v);
    }
    panel.explode_release(&mut w);
    assert_eq!(w.viewport.history.undo_len(), before + 1);
    assert_eq!(
        exploded(&w, bx),
        Some(Exploded {
            factor: 1.2,
            mode: "outward".into()
        })
    );
    panel.reassemble(&mut w);
    assert_eq!(world(&w, bx), assembled);
    w.viewport.undo();
    w.viewport.undo();
    assert_eq!(world(&w, bx), assembled);
    assert_eq!(exploded(&w, bx), None);
}
