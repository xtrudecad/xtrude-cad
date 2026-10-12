// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window tests: in an open box, a rectangle on
//! the NEXT side — and on the opposite one — starts level with the first
//! rectangle's corner, through the real viewport. (The engine half is in
//! xtrude-core.)

use crate::common;
use xtrude_app::input::ScreenPt;
use xtrude_core::Axis;

use common::window;
use glam::DVec3;
use xtrude_app::input::{Button, Modifiers, MouseEvent};
use xtrude_app::viewport::Viewport;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::SnapKind;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// An 8 × 5 × 3 box open above, a 1.5 × 1.2 rectangle on the front side.
fn open_box() -> MainWindow {
    let mut w = window();
    w.viewport.resize(1600., 900.);
    w.viewport.camera.set_aspect(1600, 900);
    let (wd, d, h) = (8.0, 5.0, 3.0);
    let m = w.viewport.scene.mesh_mut();
    m.add_face(
        &[v(0., 0., 0.), v(wd, 0., 0.), v(wd, d, 0.), v(0., d, 0.)],
        &[],
    );
    m.add_face(
        &[v(0., 0., 0.), v(wd, 0., 0.), v(wd, 0., h), v(0., 0., h)],
        &[],
    ); // front, y = 0
    m.add_face(
        &[v(wd, 0., 0.), v(wd, d, 0.), v(wd, d, h), v(wd, 0., h)],
        &[],
    ); // right, x = 8
    m.add_face(&[v(wd, d, 0.), v(0., d, 0.), v(0., d, h), v(wd, d, h)], &[]); // far, y = 5
    m.add_face(
        &[v(0., d, 0.), v(0., 0., 0.), v(0., 0., h), v(0., d, h)],
        &[],
    ); // left, x = 0
    m.add_face(
        &[
            v(2., 0., 1.),
            v(3.5, 0., 1.),
            v(3.5, 0., 2.2),
            v(2., 0., 2.2),
        ],
        &[],
    );
    w.viewport.scene.version += 1;
    w
}

fn hover(vp: &mut Viewport, world: DVec3) -> (f64, f64) {
    let px = vp.world_to_pixel(world).expect("on screen");
    vp.input.last_mouse_pos = Some(px);
    vp.process_hover(px.x, px.y, Modifiers::NONE);
    px.tuple()
}

fn click(vp: &mut Viewport, px: (f64, f64)) {
    vp.input.last_mouse_pos = Some(ScreenPt::from(px));
    vp.mouse_press(&MouseEvent::button(px.0, px.1, Button::Left, true));
}

fn snap_kind(vp: &Viewport) -> SnapKind {
    vp.inference
        .last_snap
        .as_ref()
        .map_or(SnapKind::None, |s| s.kind.clone())
}

#[test]
fn test_the_rectangle_on_the_next_side_starts_level_with_the_first_one() {
    let mut w = open_box();
    let cam = &mut w.viewport.camera;
    cam.target = v(6., 2.5, 1.5);
    cam.distance = 16.0;
    cam.pitch = 18.0_f64.to_radians();
    cam.yaw = (-55.0_f64).to_radians(); // front and right sides in view
    w.activate_tool("rectangle");
    let vp = &mut w.viewport;
    vp.encourage_point(v(3.5, 0., 2.2)); // the first rectangle's top corner
    let faces_before = vp.scene.mesh().faces.len();
    // Along the right side, 3 cm above the height: the tip says level.
    let px = hover(vp, v(8., 2.5, 2.23));
    let s = vp.inference.last_snap.clone().expect("a snap");
    assert!(
        s.kind == SnapKind::Aligned && s.axis == Some(Axis::Z),
        "{:?}",
        s.kind
    );
    assert!((s.point.x - 8.0).abs() < 1e-6 && (s.point.z - 2.2).abs() < 1e-6);
    click(vp, px);
    let far = hover(vp, v(8., 4.0, 1.0));
    click(vp, far);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), faces_before + 1);
    let new = *m.face_ids().last().expect("a face");
    let pts: Vec<DVec3> = m.face(new).outer.iter().map(|&p| m.pos(p)).collect();
    let r6 = |x: f64| (x * 1e6).round() / 1e6;
    let mut zs: Vec<f64> = pts.iter().map(|p| r6(p.z)).collect();
    zs.sort_by(f64::total_cmp);
    zs.dedup();
    let mut xs: Vec<f64> = pts.iter().map(|p| r6(p.x)).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    assert_eq!(xs, vec![8.0]); // on the right side
    assert_eq!(*zs.last().unwrap(), 2.2, "{zs:?}"); // its top level with the first
}

#[test]
fn test_the_opposite_side_too_where_no_axis_line_reaches() {
    let mut w = open_box();
    let cam = &mut w.viewport.camera;
    cam.target = v(4., 2.5, 1.5);
    cam.distance = 14.0;
    cam.pitch = 35.0_f64.to_radians();
    cam.yaw = (-100.0_f64).to_radians(); // looking in over the front side
    w.activate_tool("rectangle");
    let vp = &mut w.viewport;
    vp.encourage_point(v(3.5, 0., 2.2));
    for x in [1.2, 4.0, 6.8] {
        hover(vp, v(x, 5., 2.22));
        let s = vp.inference.last_snap.clone().expect("a snap");
        assert_eq!(s.kind, SnapKind::Aligned, "{x}");
        assert!((s.point.y - 5.0).abs() < 1e-6 && (s.point.z - 2.2).abs() < 1e-6);
    }
    // Well off the height: nothing but the side.
    hover(vp, v(4.0, 5., 1.5));
    assert_eq!(snap_kind(vp), SnapKind::OnFace);
}
