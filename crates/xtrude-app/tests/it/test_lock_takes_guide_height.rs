// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! With the line locked to the blue axis (Shift or the Up arrow), hovering a
//! guide takes its height.

use crate::common;
use xtrude_app::input::ScreenPt;
use xtrude_core::Axis;

use common::{window, MetreDocument};
use glam::DVec3;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::line::LineTool;
use xtrude_app::window::MainWindow;
use xtrude_core::snap::{SnapKind, SnapResult};
use xtrude_model::guide::Guide;

const HEIGHT: f64 = 0.825;

fn setup() -> MainWindow {
    let mut w = window();
    let v = &mut w.viewport;
    v.resize(1200., 800.);
    v.camera.set_aspect(1200, 800);
    // A horizontal guide 3 m behind the start, at the height to fetch.
    v.scene.doc.guides.push(Guide::new(
        DVec3::new(0., 3.0, HEIGHT),
        Some(DVec3::X),
        None,
    ));
    v.camera.target = DVec3::new(0., 1., 0.5);
    w.activate_tool("line");
    w.viewport.tool_mut::<LineTool>().unwrap().start_point = Some(DVec3::new(0.5, 0., 0.));
    w
}

fn hover_guide(w: &mut MainWindow, x: f64, m: Modifiers) -> SnapResult {
    let v = &mut w.viewport;
    let ScreenPt { x: px, y: py } = v.world_to_pixel(DVec3::new(x, 3.0, HEIGHT)).unwrap();
    v.build_ctx(px, py, m).and_then(|c| c.snap).expect("a snap")
}

fn on_the_start_column(s: &SnapResult) {
    assert!((s.point.z - HEIGHT).abs() < 1e-4, "{}", s.point);
    assert!(
        (s.point.x - 0.5).abs() < 1e-6 && s.point.y.abs() < 1e-6,
        "{}",
        s.point
    );
}

#[test]
fn test_shift_on_the_blue_axis_takes_the_guides_height() {
    let _m = MetreDocument::new();
    for x in [-1.0, 0.2, 0.5, 2.0] {
        let mut w = setup();
        w.viewport.inference.shift_lock = Some((DVec3::Z, [0.2, 0.3, 0.9]));
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        let s = hover_guide(&mut w, x, shift);
        assert_eq!(s.kind, SnapKind::FromPoint, "x = {x}");
        on_the_start_column(&s);
    }
}

#[test]
fn test_so_does_the_up_arrow() {
    let _m = MetreDocument::new();
    let mut w = setup();
    w.viewport.inference.axis_lock = Some(Axis::Z);
    let s = hover_guide(&mut w, 1.5, Modifiers::NONE);
    on_the_start_column(&s);
}
