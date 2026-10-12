// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A sliver face still has a unit normal (a zero normal is a wildcard plane
//! that claims every line).

use crate::common;
use common::*;
use xtrude_core::math::VecExt;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::RebuildPlaneFacesCommand;
use xtrude_model::edits::build_add_edge;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

/// `unit_or_zero` nulls short vectors; face normals must not go through
/// it.
#[test]
fn test_unit_or_zero_nulls_short_vectors_which_is_why_we_divide_by_hand() {
    assert_eq!(V3(0., 0., 1e-4).unit_or_zero().length(), 1.0);
    assert_eq!(V3(0., 0., 1e-5).unit_or_zero().length(), 0.0);
}

#[test]
fn test_a_sliver_face_still_reports_a_unit_normal() {
    let mut m = Mesh::new();
    let f = m.add_face(
        &[V(0., 0.), V(0.003, 0.), V(0.003, 0.001), V(0., 0.001)],
        &[],
    );
    let a = m.face_area(f);
    assert!(0.0 < a && a < 5e-6);
    assert!(
        m.face_newell(f).length() < 1e-5,
        "not in unit_or_zero's null band"
    );
    let n = m.face_normal(f);
    assert!(
        (n.length() - 1.0).abs() < 1e-6,
        "a zero normal is a wildcard plane"
    );
    assert!(
        (n.z.abs() - 1.0).abs() < 1e-6,
        "and it points the right way"
    );
}

#[test]
fn test_a_truly_degenerate_face_still_gets_the_documented_fallback() {
    let mut m = Mesh::new();
    let f = m.add_face(&[V(0., 0.), V(1., 0.), V(2., 0.)], &[]);
    assert_eq!(m.face_normal(f), DVec3::Z);
}

fn upper(s: &Scene) -> usize {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| (p.z - 0.5).abs() < 1e-6))
        .count()
}

#[test]
fn test_a_line_on_the_floor_leaves_the_slab_above_alone() {
    let mut s = Scene::new();
    let mut m = Mesh::new();
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(10., 0., 0.),
            V3(10., 8., 0.),
            V3(0., 8., 0.),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(0., 0., 0.5),
            V3(4., 0., 0.5),
            V3(4., 4., 0.5),
            V3(0., 4., 0.5),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(6., 0., 0.5),
            V3(10., 0., 0.5),
            V3(10., 4., 0.5),
            V3(6., 4., 0.5),
        ],
        &[],
    );
    let sliver = m.add_face(
        &[
            V3(4.0, 0.0, 0.5),
            V3(4.003, 0.0, 0.5),
            V3(4.003, 0.001, 0.5),
            V3(4.0, 0.001, 0.5),
        ],
        &[],
    );
    assert!(m.face_normal(sliver).length() > 0.5);
    let g = Group::new(add_mesh(&mut s.doc.meshes, m), Some("Plate"));
    let gid = g.id;
    s.doc.groups.push(g);
    let mut h = History::new();
    s.begin_group_edit(gid);
    let before = upper(&s);
    assert_eq!(before, 3);
    let mut cmd = build_add_edge(&s, V3(1., 1., 0.), V3(9., 1., 0.), true, None);
    h.execute(&mut s, &mut *cmd);
    assert!(h.last_error.is_none());
    assert_eq!(
        upper(&s),
        before,
        "the sliver claimed a line on another plane and the rebuild wiped it"
    );
    assert!(nfaces(&s) > 0, "the group lost every face");
}

#[test]
fn test_a_rebuild_with_no_plane_is_a_no_op_not_a_wipe() {
    let (mut s, mut h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)], &[]);
    s.mesh_mut()
        .add_face(&[V(6., 0.), V(10., 0.), V(10., 4.), V(6., 4.)], &[]);
    let before = nfaces(&s);
    h.execute(
        &mut s,
        &mut RebuildPlaneFacesCommand::new(V(1., 1.), DVec3::ZERO),
    );
    assert_eq!(nfaces(&s), before);
}
