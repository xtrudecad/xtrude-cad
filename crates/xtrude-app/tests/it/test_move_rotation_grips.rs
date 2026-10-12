// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Rotation grips on Move.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::move_tool::{rotation_grips, MoveTool};
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::Viewport;
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, oriented_bounds, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn box_group(vp: &mut Viewport, sx: f64, sy: f64, sz: f64) -> GroupId {
    let mut m = Mesh::new();
    let p = [
        v3(0., 0., 0.),
        v3(sx, 0., 0.),
        v3(sx, sy, 0.),
        v3(0., sy, 0.),
        v3(0., 0., sz),
        v3(sx, 0., sz),
        v3(sx, sy, sz),
        v3(0., sy, sz),
    ];
    for idx in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let pts: Vec<DVec3> = idx.iter().map(|&i| p[i]).collect();
        m.add_face(&pts, &[]);
    }
    let id = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(id, Some("Spacer"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    gid
}

fn obb(vp: &Viewport, g: GroupId) -> ((DVec3, DVec3, DVec3), DVec3, DVec3) {
    let m = vp.scene.doc.mesh(vp.scene.doc.find_group(g).unwrap().mesh);
    let pts: Vec<DVec3> = m.verts.iter().map(|(_, v)| v.position).collect();
    oriented_bounds(&pts, None)
}

#[test]
fn test_grips_sit_on_the_faces_that_look_at_the_eye() {
    let mut vp = stub();
    let g = box_group(&mut vp, 4.0, 1.0, 1.0);
    let grips = rotation_grips(obb(&vp, g), v3(10., -10., 10.));
    let normals: std::collections::BTreeSet<(i64, i64, i64)> = grips
        .iter()
        .map(|(_, _, n)| (n.x.round() as i64, n.y.round() as i64, n.z.round() as i64))
        .collect();
    assert_eq!(
        normals,
        [(1, 0, 0), (0, -1, 0), (0, 0, 1)].into_iter().collect()
    );
    assert_eq!(grips.len(), 12);
    for (pos, c, n) in grips {
        assert!((pos - c).dot(n).abs() < 1e-6);
        assert!(-1e-6 <= pos.x && pos.x <= 4.0 + 1e-6 && -1e-6 <= pos.y && pos.y <= 1.0 + 1e-6);
    }
}

#[test]
fn test_a_flat_group_gets_grips_only_on_its_face() {
    let pts = vec![
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 1., 0.),
        v3(0., 1., 0.),
    ];
    let grips = rotation_grips(oriented_bounds(&pts, None), v3(1., 0.5, 5.));
    assert!(!grips.is_empty() && grips.iter().all(|(_, _, n)| (n.z.abs() - 1.0).abs() < 1e-9));
}

/// A top-down stand-in: 1 m = 100 px, y flipped; the group picks inside
/// its 4 × 1 footprint.
fn setup() -> (Viewport, GroupId, MoveTool) {
    let mut vp = stub();
    let g = box_group(&mut vp, 4.0, 1.0, 1.0);
    FakeProbe::of(&mut vp).world_to_pixel(|p: DVec3| Some((p.x * 100.0, -p.y * 100.0)));
    FakeProbe::of(&mut vp).eye(Some(v3(2., 0.5, 50.)));
    FakeProbe::of(&mut vp).pick_group(move |x: f64, y: f64| {
        let (wx, wy) = (x / 100.0, -y / 100.0);
        ((0.0..=4.0).contains(&wx) && (0.0..=1.0).contains(&wy)).then_some(g)
    });
    let mut t = MoveTool::new();
    t.on_activate(&mut vp);
    (vp, g, t)
}

fn ctx(wx: f64, wy: f64) -> ToolContext {
    let mut c = ToolContext::at(v3(wx, wy, 1.0));
    c.screen = (wx * 100.0, -wy * 100.0);
    c
}

fn corners(vp: &Viewport, g: GroupId) -> Vec<(i64, i64)> {
    let m = vp.scene.doc.mesh(vp.scene.doc.find_group(g).unwrap().mesh);
    let mut out: Vec<(i64, i64)> = m
        .verts
        .iter()
        .filter(|(_, v)| v.position.z.abs() < 1e-6)
        .map(|(_, v)| {
            (
                (v.position.x * 1e4).round() as i64,
                (v.position.y * 1e4).round() as i64,
            )
        })
        .collect();
    out.sort();
    out
}

fn xs_ys(vp: &Viewport, g: GroupId) -> ((f64, f64), (f64, f64)) {
    let c = corners(vp, g);
    let xs: Vec<f64> = c.iter().map(|p| p.0 as f64 / 1e4).collect();
    let ys: Vec<f64> = c.iter().map(|p| p.1 as f64 / 1e4).collect();
    let mm = |v: &[f64]| {
        (
            v.iter().cloned().fold(f64::MAX, f64::min),
            v.iter().cloned().fold(f64::MIN, f64::max),
        )
    };
    (mm(&xs), mm(&ys))
}

const ORIGINAL: [(i64, i64); 4] = [(0, 0), (0, 10000), (40000, 0), (40000, 10000)];

#[test]
fn test_hovering_a_group_shows_its_grips_and_one_lights_up() {
    let (mut vp, _g, mut t) = setup();
    t.on_hover(&mut vp, &ctx(2.0, 0.5));
    assert_eq!(t.grips.len(), 4);
    assert!(t.hot_grip.is_none());
    let gx = 2.0 + 0.3 * 4.0;
    t.on_hover(&mut vp, &ctx(gx + 0.03, 0.5));
    assert!(t.hot_grip.is_some());
    t.on_hover(&mut vp, &ctx(8.0, 8.0));
    assert!(t.grips.is_empty());
}

#[test]
fn test_taking_a_grip_turns_the_group_about_its_centre_by_a_typed_angle() {
    let (mut vp, g, mut t) = setup();
    let gx = 2.0 + 0.3 * 4.0;
    t.on_hover(&mut vp, &ctx(gx, 0.5));
    t.on_click(&mut vp, &ctx(gx, 0.5));
    assert!(t.grip_rot.is_some());
    assert!(t.start_point.is_none());
    assert_eq!(vp.scene.doc.groups.len(), 1);
    t.on_hover(&mut vp, &ctx(2.0, 1.4));
    assert!(t.on_value(&mut vp, &ToolValue::Num(90.0)));
    assert!(t.grip_rot.is_none());
    let ((x0, x1), (y0, y1)) = xs_ys(&vp, g);
    assert!((x0 - 1.5).abs() < 1e-6 && (x1 - 2.5).abs() < 1e-6);
    assert!((y0 + 1.5).abs() < 1e-6 && (y1 - 2.5).abs() < 1e-6);
    vp.undo();
    assert_eq!(corners(&vp, g), ORIGINAL.to_vec());
}

#[test]
fn test_the_click_commits_the_live_angle_and_esc_puts_it_back() {
    let (mut vp, g, mut t) = setup();
    let gx = 2.0 + 0.3 * 4.0;
    t.on_hover(&mut vp, &ctx(gx, 0.5));
    t.on_click(&mut vp, &ctx(gx, 0.5));
    t.on_hover(&mut vp, &ctx(2.0, 3.0));
    t.on_cancel(&mut vp);
    assert!(t.grip_rot.is_none());
    assert_eq!(corners(&vp, g), ORIGINAL.to_vec());
    t.on_hover(&mut vp, &ctx(gx, 0.5));
    t.on_click(&mut vp, &ctx(gx, 0.5));
    t.on_hover(&mut vp, &ctx(2.0, 3.0));
    t.on_click(&mut vp, &ctx(2.0, 3.0));
    let ((x0, x1), _) = xs_ys(&vp, g);
    assert!((x0 - 1.5).abs() < 1e-6 && (x1 - 2.5).abs() < 1e-6);
}

#[test]
fn test_with_something_else_selected_move_shows_no_grips() {
    let (mut vp, _g, mut t) = setup();
    let other = vp.scene.mesh_mut().add_face(
        &[
            v3(9., 9., 0.),
            v3(10., 9., 0.),
            v3(10., 10., 0.),
            v3(9., 10., 0.),
        ],
        &[],
    );
    vp.scene
        .selection
        .insert(EntityRef::Face(MeshId::LOOSE, other));
    t.on_hover(&mut vp, &ctx(2.0, 0.5));
    assert!(t.grips.is_empty());
}

#[test]
fn test_a_plain_move_still_moves() {
    let (mut vp, g, mut t) = setup();
    t.on_hover(&mut vp, &ctx(0.5, 0.5));
    assert!(t.hot_grip.is_none());
    vp.scene.selection.insert(EntityRef::Group(g));
    t.on_click(&mut vp, &ctx(0.5, 0.5));
    t.on_click(&mut vp, &ctx(0.5, 2.5));
    let (_, (y0, _)) = xs_ys(&vp, g);
    assert!((y0 - 2.0).abs() < 1e-6);
}
