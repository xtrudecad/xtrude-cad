// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Right-click ▸ Select — All Connected, Bounding Edges, Same Color, Same
//! Layer.

use crate::common;
use crate::common::FakeProbe;

use std::collections::BTreeSet;

use common::{v3, window};
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::FaceHit;
use xtrude_app::window::CtxItem;
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::{Scene, SelectMode};
use xtrude_model::select_ops::{all_connected, bounding_edges, same_layer, same_material};

fn sq(s: &mut Scene, x0: f64) -> FaceId {
    s.mesh_mut().add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 1., 0., 0.),
            v3(x0 + 1., 1., 0.),
            v3(x0, 1., 0.),
        ],
        &[],
    )
}

/// Two squares sharing an edge and a third one apart.
fn pieces(s: &mut Scene) -> (FaceId, FaceId, FaceId) {
    (sq(s, 0.), sq(s, 1.), sq(s, 5.))
}

fn f(x: FaceId) -> EntityRef {
    EntityRef::Face(MeshId::LOOSE, x)
}

#[test]
fn test_all_connected_takes_the_piece_and_leaves_the_other() {
    let mut s = Scene::new();
    let (a, b, c) = pieces(&mut s);
    let got: BTreeSet<EntityRef> = all_connected(s.mesh(), MeshId::LOOSE, &[f(a)])
        .into_iter()
        .collect();
    assert!(got.contains(&f(a)) && got.contains(&f(b)) && !got.contains(&f(c)));
    assert_eq!(got.iter().filter(|e| e.is_edge()).count(), 7); // 4 + 4 − 1 shared
}

#[test]
fn test_bounding_edges_outline_the_selection_not_its_inside() {
    let mut s = Scene::new();
    let (a, b, _) = pieces(&mut s);
    let edges = bounding_edges(s.mesh(), MeshId::LOOSE, &[f(a), f(b)]);
    assert_eq!(edges.len(), 6); // the shared edge is inside
    let m = s.mesh();
    assert!(!edges.iter().any(|e| match e {
        EntityRef::Edge(_, x) => m.edge(*x).faces.contains(&a) && m.edge(*x).faces.contains(&b),
        _ => false,
    }));
    assert_eq!(bounding_edges(s.mesh(), MeshId::LOOSE, &[f(a)]).len(), 4);
}

#[test]
fn test_same_material_matches_named_and_plain_paint() {
    let mut s = Scene::new();
    let (a, b, c) = pieces(&mut s);
    for (x, col) in [
        (a, [0.8, 0.2, 0.2]),
        (c, [0.8, 0.2, 0.2]),
        (b, [0.1, 0.1, 0.9]),
    ] {
        s.mesh_mut().face_mut(x).attrs.paint.color = Some(col);
    }
    let set = |s: &Scene, x| {
        same_material(s, &[f(x)])
            .into_iter()
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(set(&s, a), [f(a), f(c)].into_iter().collect());
    // a named material wins over a colour that happens to match
    s.mesh_mut().face_mut(a).attrs.paint.mat = Some("Brick red".into());
    assert_eq!(set(&s, a), [f(a)].into_iter().collect());
    // unpainted is a material too
    let d = sq(&mut s, 8.);
    let e = sq(&mut s, 10.);
    assert_eq!(set(&s, d), [f(d), f(e)].into_iter().collect());
}

#[test]
fn test_same_layer_takes_faces_edges_and_groups_on_it() {
    let mut s = Scene::new();
    let (a, b, c) = pieces(&mut s);
    s.mesh_mut().face_mut(a).attrs.layer = Some("Shells".into());
    s.mesh_mut().face_mut(c).attrs.layer = Some("Shells".into());
    let got: BTreeSet<EntityRef> = same_layer(&s, &[f(a)]).into_iter().collect();
    assert!(got.contains(&f(a)) && got.contains(&f(c)) && !got.contains(&f(b)));
    assert!(got.iter().all(|e| e.is_face())); // edges stay on the default
}

#[test]
fn test_the_triple_click_still_selects_the_whole_piece() {
    let mut vp = common::stub();
    let (a, b, c) = pieces(&mut vp.scene);
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: a,
        placement: None,
        xform: None,
        owner: None,
    }));
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    SelectTool::new().on_triple_click(&mut vp, &ToolContext::at(v3(0., 0., 0.)));
    let sel = &vp.scene.selection;
    assert!(sel.contains(&f(a)) && sel.contains(&f(b)) && !sel.contains(&f(c)));
}

#[test]
fn test_the_right_click_menu_offers_select_and_it_grows_the_selection() {
    let mut w = window();
    let (a, b, c) = pieces(&mut w.viewport.scene);
    w.viewport.scene.select([f(a)], SelectMode::Replace);
    let sub = w
        .context_menu()
        .into_iter()
        .find_map(|i| match i {
            CtxItem::Menu(t, items) if t == "Select" => Some(items),
            _ => None,
        })
        .expect("the Select submenu");
    assert_eq!(sub.len(), 4);
    if let CtxItem::Entry { id, .. } = &sub[0] {
        w.trigger(id); // All Connected
    }
    let sel = &w.viewport.scene.selection;
    assert!(sel.contains(&f(a)) && sel.contains(&f(b)) && !sel.contains(&f(c)));
}
