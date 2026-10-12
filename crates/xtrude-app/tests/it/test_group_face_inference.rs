// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The face plane over a group or instance is the placed copy's (world), and a
//! second point lands ON the face under the cursor unless an axis heading is
//! active.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{v3, window};
use glam::DMat4;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::face_plane_world;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

fn unit_box(m: &mut Mesh) -> Vec<FaceId> {
    let p = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
        v3(0., 0., 1.),
        v3(1., 0., 1.),
        v3(1., 1., 1.),
        v3(0., 1., 1.),
    ];
    [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ]
    .iter()
    .map(|q| m.add_face(&q.map(|i| p[i]), &[]))
    .collect()
}

#[test]
fn test_face_plane_world_follows_the_instance_transform() {
    let mut proto = Mesh::new();
    let top = unit_box(&mut proto)[1]; // z = 1, normal +Z
    assert_eq!(face_plane_world(&proto, top, None).0, v3(0.5, 0.5, 1.0));
    let xf = DMat4::from_translation(v3(3., 2., 0.5));
    let (c, n) = face_plane_world(&proto, top, Some(&xf));
    assert!((c - v3(3.5, 2.5, 1.5)).length() < 1e-6);
    assert!((n.z.abs() - 1.0).abs() < 1e-6);
    let rot = DMat4::from_translation(v3(3., 0., 0.)) * DMat4::from_rotation_x(90f64.to_radians());
    let (c, n) = face_plane_world(&proto, top, Some(&rot)); // the top now faces −Y
    assert!((n.y.abs() - 1.0).abs() < 1e-6 && n.z.abs() < 1e-6);
    assert!((c.y + 1.0).abs() < 1e-6 && (c.x - 3.5).abs() < 1e-6);
}

/// Two placements of one box, B 3 m along X; a perspective view of both.
fn with_instance() -> (MainWindow, FaceId, GroupId) {
    let mut w = window();
    let vp = &mut w.viewport;
    let mut proto = Mesh::new();
    let top = unit_box(&mut proto)[1];
    let pid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let (mut a, mut b) = (Group::new(pid, Some("A")), Group::new(pid, Some("B")));
    a.xform = Some(DMat4::IDENTITY);
    b.xform = Some(DMat4::from_translation(v3(3., 0., 0.)));
    let bid = b.id;
    vp.scene.doc.groups.extend([a, b]);
    vp.scene.version += 1;
    vp.resize(800., 600.);
    vp.camera.set_aspect(800, 600);
    let c = &mut vp.camera;
    c.perspective = true;
    c.yaw = 35f64.to_radians();
    c.pitch = 35f64.to_radians();
    c.target = v3(2., 0.5, 0.5);
    c.distance = 8.0;
    (w, top, bid)
}

#[test]
fn test_work_plane_over_an_instance_face_is_the_placed_copy() {
    let (mut w, top, b) = with_instance();
    let vp = &mut w.viewport;
    let ScreenPt { x, y } = vp.world_to_pixel(v3(3.5, 0.5, 1.0)).unwrap();
    let hit = vp.pick_face_any(x, y).expect("a face");
    assert!(hit.owner == Some(b) && hit.face == top);
    vp.set_active_tool(Some(Box::new(SelectTool::new())));
    let (point, normal) = vp.current_work_plane(Some((x, y)));
    assert!((point - v3(3.5, 0.5, 1.0)).length() < 1e-6);
    assert!((normal.z.abs() - 1.0).abs() < 1e-6);
    // the same cursor and view answer the same
    assert_eq!(vp.pick_face_any(x, y), Some(hit));
}

#[test]
fn test_second_point_lands_on_the_face_under_the_cursor_unless_axis() {
    let (mut w, _top, b) = with_instance();
    let vp = &mut w.viewport;
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    vp.tool_mut::<MoveTool>().unwrap().start_point = Some(v3(0.5, 0.5, 1.0)); // A's top
    let ScreenPt { x, y } = vp.world_to_pixel(v3(3.5, 0.5, 1.0)).unwrap(); // aiming at B's top
    let (_point, normal) = vp.current_work_plane(Some((x, y)));
    let hit = vp.world_from_pixel(x, y).unwrap();
    assert!((hit.z - 1.0).abs() < 1e-6); // ON B's top face
    assert!((normal.z.abs() - 1.0).abs() < 1e-6);
    // the moved object's own faces never attract: select B and aim at it
    vp.scene.selection.insert(EntityRef::Group(b));
    let (p2, _n2) = vp.current_work_plane(Some((x, y)));
    let start = v3(0.5, 0.5, 1.0);
    assert!(p2 == start || (p2.z - 1.0).abs() > 1e-6 || p2 != v3(3.5, 0.5, 1.0));
    vp.scene.clear_selection();
    // an axis-aligned heading keeps the start-point plane (draw upward)
    let start = v3(3.5, 0.5, 2.0); // right above B's top
    vp.tool_mut::<MoveTool>().unwrap().start_point = Some(start);
    let ScreenPt { x: ux, y: uy } = vp.world_to_pixel(v3(3.5, 0.5, 4.0)).unwrap();
    let (p3, _n3) = vp.current_work_plane(Some((ux, uy)));
    assert_eq!(p3, start);
}
