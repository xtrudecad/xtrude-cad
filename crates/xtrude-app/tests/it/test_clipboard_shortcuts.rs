// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Cut, Copy and Paste shortcuts through the shell's input: egui-winit
//! turns Ctrl+X / C / V into Cut / Copy / Paste events and drops the key, and
//! the shell gives the keys back (Ctrl+Alt+V included), so the shortcuts see
//! them.

use crate::common;

use common::{v3, window};
use xtrude_app::shell::input::{FrameInput, Input, InputState};
use xtrude_app::window::MainWindow;
use xtrude_model::entity::EntityRef;

/// Feed one frame of egui events to `w` as the shell does.
fn feed(w: &mut MainWindow, ev: egui::Event, mods: egui::Modifiers) {
    let mut input = InputState::default();
    let frame = FrameInput {
        events: &[ev],
        mods,
        time_ms: 0,
        hover_pos: None,
    };
    let out = input.translate(&frame, egui::Pos2::ZERO, &|_| true, true);
    for i in out {
        if let Input::KeyPress(k) = i {
            w.key_press(&k);
        }
    }
}

fn faces(w: &MainWindow) -> usize {
    w.viewport.scene.mesh().faces.len()
}

#[test]
fn test_ctrl_c_and_ctrl_alt_v_reach_the_actions() {
    let mut w = window();
    let mid = w.viewport.scene.active_mesh_id();
    let f = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    w.viewport.scene.selection.insert(EntityRef::Face(mid, f));
    feed(&mut w, egui::Event::Copy, egui::Modifiers::CTRL);
    assert!(w.viewport.clipboard.is_some(), "Ctrl+C copied");
    w.trigger("delete_selection"); // so the paste does not merge into it
    assert_eq!(faces(&w), 0);

    feed(
        &mut w,
        egui::Event::Paste("whatever text".into()),
        egui::Modifiers::CTRL | egui::Modifiers::ALT,
    );
    assert_eq!(faces(&w), 1, "Ctrl+Alt+V pasted in place");

    feed(
        &mut w,
        egui::Event::Paste("x".into()),
        egui::Modifiers::CTRL,
    );
    let key = w.viewport.active_tool().map(|t| t.info().key.to_string());
    assert_eq!(key.as_deref(), Some("paste"), "Ctrl+V picks the copy up");
}

#[test]
fn test_ctrl_x_cuts() {
    let mut w = window();
    let mid = w.viewport.scene.active_mesh_id();
    let f = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    w.viewport.scene.selection.insert(EntityRef::Face(mid, f));
    feed(&mut w, egui::Event::Cut, egui::Modifiers::CTRL);
    assert!(w.viewport.clipboard.is_some());
    assert_eq!(faces(&w), 0);
}
