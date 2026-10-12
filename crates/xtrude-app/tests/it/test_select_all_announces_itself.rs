// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Select All bumps the scene version (the render caches key on it) and tells
//! the panels.

use crate::common;

use common::{v3, window};
use xtrude_app::viewport::ViewportEvent;

#[test]
fn test_select_all_bumps_the_version_and_notifies() {
    let mut w = window();
    w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(2., 0., 0.),
            v3(2., 2., 0.),
            v3(0., 2., 0.),
        ],
        &[],
    );
    w.pump_events();
    w.seen_events.clear();
    let before = w.viewport.scene.version;
    w.trigger("select_all");
    w.pump_events();
    assert!(!w.viewport.scene.selection.is_empty(), "nothing selected");
    assert!(w.viewport.scene.version > before);
    let last = w.seen_events.iter().rev().find_map(|e| match e {
        ViewportEvent::SceneVersionChanged(v) => Some(*v),
        _ => None,
    });
    assert_eq!(last, Some(w.viewport.scene.version)); // the tray hears about it
}
