// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The batched pick index gives the same answers as a walk over the faces,
//! seen through a plan camera. The chunk fingerprint test reads the frame
//! cache (`Viewport::frame_data`).

use crate::common;

use common::{sized_viewport, v3};
use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::commands::MakeGroupCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};

/// A plan view over `lo..hi`.
fn plan(lo: DVec3, hi: DVec3) -> Viewport {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("top");
    vp.camera.fit_to(lo, hi, 1.3);
    vp
}

fn px(vp: &Viewport, p: DVec3) -> (f64, f64) {
    vp.world_to_pixel(p).expect("on screen").tuple()
}

fn square(m: &mut Mesh, x0: f64, y0: f64, s: f64) -> xtrude_core::FaceId {
    m.add_face(
        &[
            v3(x0, y0, 0.),
            v3(x0 + s, y0, 0.),
            v3(x0 + s, y0 + s, 0.),
            v3(x0, y0 + s, 0.),
        ],
        &[],
    )
}

fn grouped(vp: &mut Viewport) {
    let f = square(vp.scene.mesh_mut(), -2., -2., 4.);
    let mk = MakeGroupCommand::new(vp.scene.mesh(), &[f], &[], false, None);
    vp.run(mk);
}

#[test]
fn test_pick_face_prefers_smallest_coplanar() {
    let mut vp = plan(v3(-5., -5., 0.), v3(5., 5., 0.));
    let big = square(vp.scene.mesh_mut(), -5., -5., 10.);
    let small = square(vp.scene.mesh_mut(), -1., -1., 2.);
    assert_ne!(big, small);
    vp.scene.version += 1;
    let p = px(&vp, DVec3::ZERO);
    let (_, hit) = vp.pick_face(p.0, p.1).expect("a face");
    // Both overlap at the origin at the same depth: the smallest wins.
    assert!(hit == small || vp.scene.mesh().face_area(hit) < 5.0);
}

#[test]
fn test_pick_face_any_reaches_group_faces() {
    let mut vp = plan(v3(-2., -2., 0.), v3(2., 2., 0.));
    grouped(&mut vp);
    let g = vp.scene.doc.groups[0].clone();
    let p = px(&vp, DVec3::ZERO);
    let hit = vp.pick_face_any(p.0, p.1).expect("a face");
    assert_eq!(hit.owner, Some(g.id));
    assert!(vp.scene.doc.mesh(g.mesh).has_face(hit.face) && hit.mesh == g.mesh);
    assert!(vp.pick_face(p.0, p.1).is_none()); // loose only sees nothing
}

#[test]
fn test_pick_index_refreshes_on_scene_change() {
    let mut vp = plan(v3(-2., -2., 0.), v3(2., 2., 0.));
    let p = px(&vp, DVec3::ZERO);
    assert!(vp.pick_face(p.0, p.1).is_none());
    square(vp.scene.mesh_mut(), -2., -2., 4.);
    vp.scene.version += 1;
    assert!(vp.pick_face(p.0, p.1).is_some());
}

#[test]
fn test_snap_reaches_group_geometry() {
    let mut vp = plan(v3(-2., -2., 0.), v3(2., 2., 0.));
    grouped(&mut vp);
    let corner = v3(2., 2., 0.);
    let p = px(&vp, corner);
    let got = vp.pick_vertex(p.0, p.1).expect("the corner");
    assert!((got - corner).length() < 1e-4);
    let pseudo = vp.nearby_group_edges(p.0, p.1, 48.0, 48);
    assert!(
        !pseudo.is_empty(),
        "group edges near the cursor must feed the snap engine"
    );
    assert!(vp.snap_scene(Some(p)).len() >= pseudo.len());
    assert!(vp
        .nearby_group_edges(-10_000., -10_000., 48.0, 48)
        .is_empty());
}

#[test]
fn test_instance_groups_pick_through_transformed_chunks() {
    let mut vp = plan(v3(0., -5., 0.), v3(25., 5., 0.));
    let mut proto = Mesh::new();
    square(&mut proto, 0., 0., 1.);
    let pid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let mut g1 = Group::new(pid, Some("i1"));
    g1.xform = Some(DMat4::IDENTITY);
    let mut g2 = Group::new(pid, Some("i2"));
    g2.xform = Some(DMat4::from_translation(v3(10., 0., 0.)));
    let (i1, i2) = (g1.id, g2.id);
    vp.scene.doc.groups.extend([g1, g2]);
    vp.scene.version += 1;
    let owner = |vp: &mut Viewport, x: f64| {
        let p = px(vp, v3(x, 0.5, 0.));
        vp.pick_face_any(p.0, p.1).map(|h| (h.owner, h.mesh))
    };
    assert_eq!(owner(&mut vp, 0.5), Some((Some(i1), pid)));
    assert_eq!(owner(&mut vp, 10.5), Some((Some(i2), pid)));
    assert_eq!(owner(&mut vp, 5.0), None); // the empty gap between
    vp.scene.doc.find_group_mut(i2).unwrap().xform = Some(DMat4::from_translation(v3(20., 0., 0.)));
    vp.scene.version += 1;
    assert_eq!(owner(&mut vp, 10.5), None);
    assert_eq!(owner(&mut vp, 20.5).map(|o| o.0), Some(Some(i2)));
    assert_eq!(owner(&mut vp, 0.5).map(|o| o.0), Some(Some(i1)));
}

#[test]
fn test_selected_group_offers_its_box_corners_to_the_snap_engine() {
    let mut vp = plan(v3(-2., -2., 0.), v3(2., 2., 0.));
    grouped(&mut vp);
    let g = vp.scene.doc.groups[0].id;
    vp.scene.selection.clear(); // grouping leaves it selected
    assert!(vp.selection_box_points(&[]).is_empty());
    vp.scene.selection.insert(EntityRef::Group(g));
    let pts = vp.selection_box_points(&[]);
    assert_eq!(pts.len(), 8); // the box's corners
    assert!(pts.iter().all(|p| (p.a - p.b).length() < 1e-9));
    let corners: std::collections::BTreeSet<(i64, i64, i64)> = pts
        .iter()
        .map(|p| {
            (
                (p.a.x * 1e3).round() as i64,
                (p.a.y * 1e3).round() as i64,
                (p.a.z * 1e3).round() as i64,
            )
        })
        .collect();
    assert!(corners.contains(&(2000, 2000, 0)) && corners.contains(&(-2000, -2000, 0)));
    // and they reach the engine even with no cursor position to look near
    assert!(vp.snap_scene(None).len() >= 8);
}

#[test]
fn test_group_chunk_fingerprint_invalidation() {
    // The cached drawing must refresh when the group's content changes —
    // paint (attrs) and move (positions) both — and an untouched group
    // keeps it (that reuse is what makes drawing beside a 17k-face import
    // instant). Xtrude CAD caches the whole frame, keyed by the scene
    // version and the active mesh's mutation serial: the group is opened,
    // as the tools edit it, and its mesh touched directly.
    let mut vp = plan(DVec3::new(-3., -3., 0.), DVec3::new(3., 3., 0.));
    grouped(&mut vp);
    let g = vp.scene.doc.groups[0].id;
    vp.scene.begin_group_edit(g);
    let generation = |vp: &mut Viewport| vp.frame_data([200, 200], false).scene_faces.generation;
    let fp0 = generation(&mut vp);
    assert_eq!(generation(&mut vp), fp0); // stable when untouched
    let gf = vp.scene.mesh().face_ids()[0];
    vp.scene.mesh_mut().face_mut(gf).attrs.paint.color = Some([0.5, 0.2, 0.2]);
    let fp_paint = generation(&mut vp);
    assert_ne!(fp_paint, fp0); // paint invalidates
    for v in vp.scene.mesh().vertex_ids() {
        vp.scene.mesh_mut().move_vertex(v, DVec3::new(1., 0., 0.));
    }
    assert_ne!(generation(&mut vp), fp_paint); // move invalidates
}
