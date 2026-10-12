// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Offset keeps the face's paint and layer on both halves, and a refusal says
//! why.

use crate::common;

use common::{stub, MetreDocument};
use glam::DVec3;
use xtrude_app::tools::offset::OffsetTool;
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_core::measure::face_net_area;
use xtrude_core::topology::max_offset_distance;
use xtrude_core::FaceId;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn slab(vp: &mut Viewport) -> FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(10., 0.), v(10., 6.), v(0., 6.)], &[])
}

fn offset(vp: &mut Viewport, face: FaceId, d: f64) {
    let mut t = OffsetTool::new();
    let m = vp.scene.mesh();
    t.base_face = Some((MeshId::LOOSE, face));
    t.lp = m.face_positions(face);
    t.normal = Some(m.face_normal(face));
    t.distance = d;
    t.commit_now(vp);
}

fn said(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn test_both_halves_keep_the_material() {
    let mut vp = stub();
    let f = slab(&mut vp);
    {
        let a = &mut vp.scene.mesh_mut().face_mut(f).attrs;
        a.paint.mat = Some("Slate grey".into());
        a.layer = Some("Plates".into());
    }
    offset(&mut vp, f, 1.0);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    for f in m.face_ids() {
        assert_eq!(m.face(f).attrs.paint.mat.as_deref(), Some("Slate grey"));
        assert_eq!(m.face(f).attrs.layer.as_deref(), Some("Plates"));
    }
}

#[test]
fn test_the_two_halves_do_not_share_one_attrs_dict() {
    let mut vp = stub();
    let f = slab(&mut vp);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.5, 0.5, 0.5]);
    offset(&mut vp, f, 1.0);
    let ids = vp.scene.mesh().face_ids();
    let (ring, inner) = (ids[0], ids[1]);
    vp.scene.mesh_mut().face_mut(inner).attrs.paint.color = Some([0.9, 0.2, 0.1]);
    assert_eq!(
        vp.scene.mesh().face(ring).attrs.paint.color,
        Some([0.5, 0.5, 0.5])
    );
}

#[test]
fn test_an_unpainted_face_still_offsets() {
    let mut vp = stub();
    let f = slab(&mut vp);
    offset(&mut vp, f, 1.0);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 2);
    assert!(m.face_ids().into_iter().all(|f| m.face(f).attrs.is_empty()));
}

#[test]
fn test_the_base_face_is_gone_not_left_underneath() {
    let mut vp = stub();
    let f = slab(&mut vp);
    offset(&mut vp, f, 1.0);
    let m = vp.scene.mesh();
    assert!(!m.has_face(f));
    let ring = m
        .face_ids()
        .into_iter()
        .find(|&f| !m.face(f).holes.is_empty())
        .unwrap();
    let inner = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face(f).holes.is_empty())
        .unwrap();
    let total = face_net_area(m, ring) + face_net_area(m, inner);
    assert!(((total * 1e6).round() / 1e6 - 60.0).abs() < 1e-9);
}

#[test]
fn test_undo_puts_the_painted_face_back() {
    let mut vp = stub();
    let f = slab(&mut vp);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.5, 0.5, 0.5]);
    offset(&mut vp, f, 1.0);
    vp.undo();
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 1);
    assert_eq!(
        m.face(m.face_ids()[0]).attrs.paint.color,
        Some([0.5, 0.5, 0.5])
    );
}

fn rim(vp: &mut Viewport, width: f64) -> FaceId {
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2.4, 0.), v(2.4, width), v(0., width)], &[])
}

#[test]
fn test_an_impossible_offset_says_so_instead_of_going_silent() {
    let mut vp = stub();
    let f = rim(&mut vp, 0.2);
    vp.take_events();
    offset(&mut vp, f, 0.10);
    assert_eq!(
        vp.scene.mesh().faces.len(),
        1,
        "nothing should have been built"
    );
    assert!(!said(&mut vp).is_empty(), "the tool refused without a word");
}

#[test]
fn test_the_message_names_the_room_that_is_left() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let f = rim(&mut vp, 0.5);
    let m = vp.scene.mesh();
    let room = max_offset_distance(&m.face_positions(f), m.face_normal(f), 1.0, 10.0);
    assert!(0.2 < room && room < 0.26, "{room}");
    vp.take_events();
    offset(&mut vp, f, 0.40);
    let s = said(&mut vp);
    assert!(!s.is_empty() && s[0].contains("0.2"), "{s:?}");
}

#[test]
fn test_a_feasible_offset_stays_quiet_and_builds() {
    let mut vp = stub();
    let f = rim(&mut vp, 1.0);
    vp.take_events();
    offset(&mut vp, f, 0.10);
    assert_eq!(vp.scene.mesh().faces.len(), 2);
    assert!(said(&mut vp).is_empty());
}
