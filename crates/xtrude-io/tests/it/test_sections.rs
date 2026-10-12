// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Section planes: entity, commands, persistence and scene (the tool tests
//! live in xtrude-app).

use crate::common;
use common::*;
use serde_json::json;
use xtrude_core::camera::OrbitCamera;
use xtrude_model::commands::{
    DeleteSectionPlanesCommand, MoveSectionPlanesCommand, PlaceSectionPlaneCommand,
    ReverseSectionPlaneCommand, RotateSectionPlanesCommand, SetActiveSectionCommand,
};
use xtrude_model::history::{Command, History};
use xtrude_model::ids::ObjId;
use xtrude_model::saved_views::SavedView;
use xtrude_model::scene::Scene;
use xtrude_model::section::SectionPlane;
use xtrude_model::style::Style;

fn sp(p: (f64, f64, f64), n: (f64, f64, f64), name: &str, symbol: &str) -> SectionPlane {
    SectionPlane::new(
        v3(p.0, p.1, p.2),
        v3(n.0, n.1, n.2),
        name,
        symbol,
        false,
        None,
    )
}

fn plane(s: &Scene, id: ObjId) -> &SectionPlane {
    s.doc.section_planes.iter().find(|p| p.id == id).unwrap()
}

fn active(s: &Scene) -> Option<ObjId> {
    s.active_section().map(|p| p.id)
}

fn exec(s: &mut Scene, h: &mut History, mut c: impl Command) {
    h.execute(s, &mut c);
}

#[test]
fn test_plane_side_flip_and_roundtrip() {
    let mut p = sp((0., 0., 1.), (0., 0., 1.), "Top", "1");
    assert!(p.side(v3(0., 0., 3.)) > 0.0); // above = hidden side
    assert!(p.side(v3(0., 0., 0.)) < 0.0);
    p.flip(); // the usual Reverse
    assert!(p.side(v3(0., 0., 3.)) < 0.0);
    let again = SectionPlane::from_json(&p.to_json()).unwrap();
    assert!(again.name == "Top" && again.symbol == "1");
    assert_eq!(again.uid, p.uid);
    assert!((again.normal - p.normal).length() < 1e-9);
    let fresh = sp((0., 0., 0.), (1., 0., 0.), "", "");
    assert_ne!(fresh.uid, p.uid); // loaded uids never collide
}

#[test]
fn test_place_activates_and_keeps_one_active() {
    let mut s = Scene::new();
    let mut h = History::new();
    let a = sp((0., 0., 1.), (0., 0., 1.), "", "");
    let b = sp((0., 0., 2.), (0., 0., 1.), "", "");
    let (ia, ib) = (a.id, b.id);
    exec(&mut s, &mut h, PlaceSectionPlaneCommand::new(a));
    assert_eq!(active(&s), Some(ia)); // placing = active cut
    exec(&mut s, &mut h, PlaceSectionPlaneCommand::new(b));
    assert_eq!(active(&s), Some(ib)); // one active per context
    assert!(!plane(&s, ia).active);
    assert!(h.undo(&mut s));
    assert_eq!(
        s.doc
            .section_planes
            .iter()
            .map(|p| p.id)
            .collect::<Vec<_>>(),
        vec![ia]
    );
    assert_eq!(active(&s), Some(ia)); // previous active restored

    exec(&mut s, &mut h, SetActiveSectionCommand { id: None });
    assert_eq!(active(&s), None);
    assert!(h.undo(&mut s));
    assert_eq!(active(&s), Some(ia));

    exec(&mut s, &mut h, ReverseSectionPlaneCommand { id: ia });
    assert!(plane(&s, ia).side(v3(0., 0., 3.)) < 0.0);
    exec(
        &mut s,
        &mut h,
        MoveSectionPlanesCommand {
            ids: vec![ia],
            delta: v3(0., 0., 5.),
        },
    );
    assert!((plane(&s, ia).point.z - 6.0).abs() < 1e-9);
    exec(
        &mut s,
        &mut h,
        RotateSectionPlanesCommand {
            ids: vec![ia],
            centre: v3(0., 0., 0.),
            axis: v3(1., 0., 0.),
            deg: 90.0,
        },
    );
    assert!((plane(&s, ia).normal.y.abs() - 1.0).abs() < 1e-6);
    exec(&mut s, &mut h, DeleteSectionPlanesCommand { ids: vec![ia] });
    assert!(s.doc.section_planes.is_empty());
    assert!(h.undo(&mut s));
    assert_eq!(
        s.doc
            .section_planes
            .iter()
            .map(|p| p.id)
            .collect::<Vec<_>>(),
        vec![ia]
    );
}

#[test]
fn test_igz_roundtrip_and_scene_recall() {
    let mut s = Scene::new();
    s.mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(1., 0., 0.))
        .unwrap();
    let a = sp((0., 0., 1.), (0., 0., 1.), "Top", "1");
    let b = sp((5., 0., 0.), (1., 0., 0.), "Cut A", "A");
    let ib = b.id;
    s.doc.section_planes.push(a);
    s.doc.section_planes.push(b);
    s.set_active_section(Some(ib));
    s.show_section_planes = false;
    let mut cam = OrbitCamera::default();
    let view = SavedView::capture("Cut", &s, &cam);
    s.saved_views.push(view);

    let mut fresh = roundtrip(&s);
    let names: Vec<&str> = fresh
        .doc
        .section_planes
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["Top", "Cut A"]);
    assert_eq!(fresh.active_section().unwrap().symbol, "A");
    assert!(!fresh.show_section_planes);
    assert!(fresh.show_section_cuts);

    // The saved view recalls the section state (scenes).
    fresh.set_active_section(None);
    fresh.show_section_planes = true;
    let view = fresh.saved_views[0].clone();
    view.apply(&mut fresh, &mut cam);
    assert_eq!(fresh.active_section().unwrap().symbol, "A");
    assert!(!fresh.show_section_planes);
}

#[test]
fn test_style_carries_section_fill() {
    let mut st = Style::default();
    assert!(st.section_fill); // the usual default: on
    st.section_fill = false;
    st.section_fill_color = [0.1, 0.2, 0.3];
    let again = Style::from_json(&st.to_json());
    assert!(!again.section_fill);
    assert_eq!(again.section_fill_color, [0.1, 0.2, 0.3]);
}

#[test]
fn test_scene_made_before_any_section_switches_the_cut_off() {
    let mut s = Scene::new();
    s.mesh_mut()
        .add_edge(v3(0., 0., 0.), v3(1., 0., 0.))
        .unwrap();
    let mut cam = OrbitCamera::default();
    let top = SavedView::capture("Top", &s, &cam); // no planes yet
    assert_eq!(
        serde_json::Value::Object(top.section.clone().unwrap()),
        json!({"active": null, "planes_shown": true, "cuts_shown": true})
    );
    s.saved_views.push(top.clone());
    let cut = sp((5., 0., 0.), (1., 0., 0.), "Cut A", "A");
    let ic = cut.id;
    s.doc.section_planes.push(cut);
    s.set_active_section(Some(ic));
    let cut_view = SavedView::capture("Cut A", &s, &cam);
    s.saved_views.push(cut_view.clone());

    top.apply(&mut s, &mut cam);
    assert_eq!(active(&s), None);
    cut_view.apply(&mut s, &mut cam);
    assert_eq!(active(&s), Some(ic));

    // The separation survives the document round trip.
    let mut fresh = roundtrip(&s);
    assert!(fresh.active_section().is_some());
    let v0 = fresh.saved_views[0].clone();
    v0.apply(&mut fresh, &mut cam);
    assert!(fresh.active_section().is_none());

    // A pre-sections view (no entry) still leaves the live cut alone.
    let legacy = SavedView::from_json(&json!({"name": "Old", "target": [0, 0, 0],
                                              "distance": 10.0, "yaw": 0.0, "pitch": 0.0}));
    assert!(legacy.section.is_none());
    let first = fresh.doc.section_planes[0].id;
    fresh.set_active_section(Some(first));
    legacy.apply(&mut fresh, &mut cam);
    assert!(fresh.active_section().is_some());
}
