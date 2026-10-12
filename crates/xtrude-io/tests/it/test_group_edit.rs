// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Document tests: the edit-inside context, the GLB importer it guards, and
//! the oriented selection box. The viewport span/VBO tests belong to
//! xtrude-render; the rest-mode setting lives in xtrude-app.

use crate::common;
use common::*;
use xtrude_core::DVec3;
use xtrude_core::Mesh;
use xtrude_model::commands::{AddFaceCommand, MakeGroupCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::group::{frame_from_points, oriented_bounds, oriented_box_corners};
use xtrude_model::history::{Command, History};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::Scene;

fn rect(s: &mut Scene, h: &mut History, x0: f64, y0: f64, x1: f64, y1: f64) {
    let pts = vec![
        v3(x0, y0, 0.),
        v3(x1, y0, 0.),
        v3(x1, y1, 0.),
        v3(x0, y1, 0.),
    ];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (pts[i], pts[(i + 1) % 4])).collect();
    let face: Box<dyn Command> = Box::new(AddFaceCommand::new(pts));
    let mut cmd = build_add_edges(s, &segs, false, vec![face], None);
    h.execute(s, &mut *cmd);
}

fn grouped_scene() -> (Scene, History, GroupId) {
    let mut s = Scene::new();
    let mut h = History::new();
    rect(&mut s, &mut h, 0., 0., 4., 4.);
    let (f, e) = (s.mesh().face_ids(), s.mesh().edge_ids());
    let mut mk = MakeGroupCommand::new(s.mesh(), &f, &e, false, None);
    h.execute(&mut s, &mut mk);
    let g = s.doc.groups[0].id;
    (s, h, g)
}

fn group_faces(s: &Scene, g: GroupId) -> usize {
    s.doc.mesh(s.doc.find_group(g).unwrap().mesh).faces.len()
}

#[test]
fn test_enter_edit_swaps_mesh_and_exit_restores() {
    let (mut s, _h, g) = grouped_scene();
    let gm = s.doc.find_group(g).unwrap().mesh;
    s.begin_group_edit(g);
    assert_eq!(s.active_mesh_id(), gm);
    assert_eq!(s.edit_group(), Some(g));
    assert!(std::ptr::eq(s.loose_mesh(), s.doc.mesh(MeshId::LOOSE)));
    s.end_group_edit();
    assert_eq!(s.active_mesh_id(), MeshId::LOOSE);
    assert!(s.edit_group().is_none());
}

#[test]
fn test_drawing_inside_lands_in_the_group() {
    let (mut s, mut h, g) = grouped_scene();
    s.begin_group_edit(g);
    rect(&mut s, &mut h, 1., 1., 2., 2.); // drawn INSIDE the context
    s.end_group_edit();
    assert_eq!(s.mesh().faces.len(), 0); // loose mesh untouched
    assert_eq!(group_faces(&s, g), 2); // the group gained the rect
}

#[test]
fn test_undo_after_exiting_restores_the_group_not_the_loose_mesh() {
    let (mut s, mut h, g) = grouped_scene();
    rect(&mut s, &mut h, 10., 10., 12., 12.); // loose slab (outside)
    s.begin_group_edit(g);
    rect(&mut s, &mut h, 1., 1., 2., 2.); // inside the group
    s.end_group_edit();
    let loose = s.mesh().faces.len();
    assert_eq!(group_faces(&s, g), 2);
    assert!(h.undo(&mut s)); // undoes the INSIDE rect
    assert_eq!(group_faces(&s, g), 1); // group restored
    assert_eq!(s.mesh().faces.len(), loose); // loose mesh untouched
    assert!(h.redo(&mut s));
    assert_eq!(group_faces(&s, g), 2);
}

#[test]
fn test_render_views_do_not_duplicate_while_editing() {
    let (mut s, mut h, g) = grouped_scene();
    rect(&mut s, &mut h, 10., 10., 12., 12.); // loose slab
    let total = s.render_faces().len();
    s.begin_group_edit(g);
    assert_eq!(s.render_faces().len(), total); // same faces, no dupes
    s.end_group_edit();
}

#[test]
fn test_clear_scene_exits_the_context() {
    let (mut s, _h, g) = grouped_scene();
    s.begin_group_edit(g);
    s.clear();
    assert!(s.edit_group().is_none());
    assert_eq!(s.mesh().faces.len(), 0);
}

#[test]
fn test_a_glb_component_still_imports() {
    let mut s = Scene::new();
    let mut h = History::new();
    rect(&mut s, &mut h, 0., 0., 2., 1.);
    let tmp = TmpDir::new();
    let path = tmp.join("part.glb");
    xtrude_io::gltf::save_glb(&s, &path).unwrap();
    let mut temp = Scene::new();
    xtrude_io::glb::load_glb(&mut temp, &path, None).unwrap();
    assert!(!temp.doc.groups.is_empty());
    assert!(!temp.doc.mesh(temp.doc.groups[0].mesh).faces.is_empty());
}

// ---- oriented selection box ------------------------------------------------

/// A flat 4x1 slab lying in Z=0, turned `deg` about Z.
fn rotated_slab(deg: f64) -> Mesh {
    let a = deg.to_radians();
    let (ca, sa) = (a.cos(), a.sin());
    let pts: Vec<DVec3> = [(0., 0.), (4., 0.), (4., 1.), (0., 1.)]
        .iter()
        .map(|&(x, y)| v3(x * ca - y * sa, x * sa + y * ca, 0.))
        .collect();
    let mut m = Mesh::new();
    m.add_face(&pts, &[]);
    m
}

fn positions(m: &Mesh) -> Vec<DVec3> {
    m.verts.iter().map(|(_, v)| v.position).collect()
}

fn own_bounds(m: &Mesh) -> ((DVec3, DVec3, DVec3), DVec3, DVec3) {
    let pos = positions(m);
    oriented_bounds(&pos, Some(frame_from_points(&pos)))
}

fn sides(lo: DVec3, hi: DVec3) -> Vec<f64> {
    let mut s = vec![hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
    s.sort_by(f64::total_cmp);
    s
}

#[test]
fn test_oriented_bounds_wrap_a_rotated_slab_tightly() {
    let (_f, lo, hi) = own_bounds(&rotated_slab(35.0));
    let s = sides(lo, hi);
    assert!(s[0].abs() < 1e-9);
    assert!((s[1] - 1.0).abs() < 1e-9); // the slab's own 4 x 1 x 0
    assert!((s[2] - 4.0).abs() < 1e-9);
}

#[test]
fn test_derived_frame_finds_the_true_minimum() {
    let m = rotated_slab(23.5);
    let u = frame_from_points(&positions(&m)).0;
    let ang = u.y.atan2(u.x).to_degrees().rem_euclid(90.0);
    assert!((ang - 23.5).abs().min((ang - 66.5).abs()) < 1e-6);
}

#[test]
fn test_world_aligned_bounds_would_wrap_mostly_air() {
    let m = rotated_slab(35.0);
    let pos = positions(&m);
    let (xs, ys): (Vec<f64>, Vec<f64>) = pos.iter().map(|p| (p.x, p.y)).unzip();
    let span = |v: &[f64]| {
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    let world_area = span(&xs) * span(&ys);
    let (_f, lo, hi) = own_bounds(&m);
    let s = sides(lo, hi);
    let own_area = s[1] * s[2]; // 4 x 1
    assert!((own_area - 4.0).abs() < 1e-9);
    assert!(world_area > 2.5 * own_area);
}

#[test]
fn test_oriented_bounds_match_world_axes_when_the_object_does() {
    let (frame, lo, hi) = own_bounds(&rotated_slab(0.0));
    let s = sides(lo, hi);
    assert!((s[1] - 1.0).abs() < 1e-9 && (s[2] - 4.0).abs() < 1e-9);
    for axis in [frame.0, frame.1, frame.2] {
        let mut comps: Vec<i64> = [axis.x, axis.y, axis.z]
            .iter()
            .map(|c| (c.abs() * 1e6).round() as i64)
            .collect();
        comps.sort();
        assert_eq!(comps, vec![0, 0, 1_000_000]); // each axis a world axis
    }
}

#[test]
fn test_box_corners_lie_on_the_geometry_of_a_rotated_slab() {
    let (f, lo, hi) = own_bounds(&rotated_slab(35.0));
    let corners = oriented_box_corners(f, lo, hi);
    assert_eq!(corners.len(), 8);
    assert!(corners.iter().all(|c| c.z.abs() < 1e-9));
    let keys: std::collections::BTreeSet<(i64, i64)> = corners
        .iter()
        .map(|c| ((c.x * 1e6).round() as i64, (c.y * 1e6).round() as i64))
        .collect();
    assert_eq!(keys.len(), 4); // the slab's corners, each twice
}
