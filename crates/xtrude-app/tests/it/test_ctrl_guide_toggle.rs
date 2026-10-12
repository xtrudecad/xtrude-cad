// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Ctrl on the Measure tool (three modes) and the Protractor (guides on/off) is a
//! MODE, reset on pickup; Rotate keeps Ctrl for its copy; a Ctrl that was part
//! of a shortcut toggles nothing; the status bar keeps the Ctrl clause up.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3, viewport, window};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::measure::{MeasureMode, MeasureTool};
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::viewport::{PickedEdge, Viewport, ViewportEvent};
use xtrude_app::window::hints::hint_for;
use xtrude_app::window::status_bar::StatusBar;
use xtrude_core::snap::LinearMode;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn edge(a: DVec3, b: DVec3) -> PickedEdge {
    PickedEdge {
        a,
        b,
        edge: None,
        in_group: false,
        context: None,
        group: None,
        soft: false,
        curve: None,
    }
}

fn vp_with(e: Option<PickedEdge>) -> Viewport {
    let mut vp = stub();
    let a = e.clone();
    FakeProbe::of(&mut vp).pick_edge(move |_, _| a.clone());
    FakeProbe::of(&mut vp).pick_edge_any(move |_, _| e.clone());
    FakeProbe::of(&mut vp).pick_guide(|_, _| None);
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    vp
}

fn x_edge_vp() -> Viewport {
    vp_with(Some(edge(v(0., 0.), v(4., 0.))))
}

fn ctx(x: f64, y: f64) -> ToolContext {
    ToolContext::at(v(x, y))
}

/// A Ctrl tap: the Measure tool switches on the release of a Ctrl pressed alone;
/// the Protractor and Rotate act on the press.
fn tap(tool: &mut dyn Tool, vp: &mut Viewport) -> bool {
    // what the viewport's press records
    vp.key_press(&KeyEvent::new(Key::Control, "", Modifiers::CTRL));
    let pressed = tool.on_key(vp, Key::Control, Modifiers::NONE);
    match tool.on_key_release(vp, Key::Control) {
        None => pressed,
        Some(r) => r,
    }
}

fn flashed(vp: &mut Viewport) -> Vec<String> {
    vp.outbox
        .events
        .drain(..)
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

// ── Measure ───────────────────────────────────────────────────────────────

#[test]
fn test_the_measure_tool_leaves_a_guide_by_default() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    tool.on_click(&mut vp, &ctx(1., 0.));
    tool.on_click(&mut vp, &ctx(1., 2.));
    assert_eq!(vp.scene.doc.guides.len(), 1);
}

#[test]
fn test_ctrl_cycles_the_measure_tool_through_the_three_classic_modes() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    assert_eq!(tool.mode, MeasureMode::Line);
    for expected in [MeasureMode::Point, MeasureMode::Measure, MeasureMode::Line] {
        assert!(tap(&mut tool, &mut vp));
        assert_eq!(tool.mode, expected);
    }
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_the_middle_mode_drops_a_guide_POINT() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    tap(&mut tool, &mut vp); // → points
    tool.on_click(&mut vp, &ctx(1., 0.));
    tool.on_click(&mut vp, &ctx(1., 2.));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    let g = &vp.scene.doc.guides[0];
    assert!(!g.is_line(), "a POINT, not a line");
    assert!((g.point.y - 2.0).abs() < 1e-6);
}

#[test]
fn test_a_typed_distance_places_the_point_exactly() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    tap(&mut tool, &mut vp); // → points
    tool.on_click(&mut vp, &ctx(0., 0.));
    tool.on_hover(&mut vp, &ctx(0., 1.)); // pointing up +Y
    assert!(tool.on_value(&mut vp, &ToolValue::Num(2.5)));
    assert_eq!(vp.scene.doc.guides.len(), 1);
    assert!((vp.scene.doc.guides[0].point.y - 2.5).abs() < 1e-6);
}

#[test]
fn test_ctrl_turns_the_measure_tool_into_a_plain_ruler() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    tap(&mut tool, &mut vp); // → points
    tap(&mut tool, &mut vp); // → measure
    assert!(!tool.cursor_plus());
    tool.on_click(&mut vp, &ctx(1., 0.));
    tool.on_click(&mut vp, &ctx(1., 2.));
    assert!(
        vp.scene.doc.guides.is_empty(),
        "measure only: nothing left behind"
    );
    assert!(tool.measured.is_some(), "…but it did measure");
}

#[test]
fn test_picking_the_tool_up_starts_in_guide_mode() {
    let mut vp = x_edge_vp();
    let mut measure = MeasureTool::new();
    while measure.mode != MeasureMode::Measure {
        tap(&mut measure, &mut vp);
    }
    measure.on_activate(&mut vp);
    assert_eq!(measure.mode, MeasureMode::Line);
    let mut pro = ProtractorTool::new();
    while pro.guides {
        tap(&mut pro, &mut vp);
    }
    pro.on_activate(&mut vp);
    assert!(pro.guides);
}

#[test]
fn test_the_cursor_says_which_mode_it_is_in() {
    let mut vp = vp_with(None);
    let mut measure = MeasureTool::new();
    measure.on_activate(&mut vp);
    assert!(measure.cursor_plus());
    while measure.mode != MeasureMode::Measure {
        tap(&mut measure, &mut vp);
    }
    assert!(!measure.cursor_plus());
    let mut pro = ProtractorTool::new();
    pro.on_activate(&mut vp);
    assert!(pro.cursor_plus());
    while pro.guides {
        tap(&mut pro, &mut vp);
    }
    assert!(!pro.cursor_plus());
}

#[test]
fn test_the_measure_tool_toggle_survives_the_operation() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    tap(&mut tool, &mut vp); // → points
    tap(&mut tool, &mut vp); // → measure
    tool.on_click(&mut vp, &ctx(1., 0.));
    tool.on_click(&mut vp, &ctx(1., 2.));
    tool.on_click(&mut vp, &ctx(1., 0.)); // a second measurement
    tool.on_click(&mut vp, &ctx(1., 3.));
    assert!(vp.scene.doc.guides.is_empty());
    tap(&mut tool, &mut vp); // …until it comes round
    assert_eq!(tool.mode, MeasureMode::Line);
}

// ── Protractor ────────────────────────────────────────────────────────────

fn measure(vp: &mut Viewport, tool: &mut ProtractorTool, deg: f64) {
    tool.on_click(vp, &ctx(0., 0.)); // the vertex
    tool.on_click(vp, &ctx(2., 0.)); // the base arm
    let r = 2.0;
    let a = deg.to_radians();
    tool.on_click(vp, &ctx(r * a.cos(), r * a.sin()));
}

#[test]
fn test_the_protractor_leaves_a_guide_by_default() {
    let mut vp = vp_with(None);
    measure(&mut vp, &mut ProtractorTool::new(), 45.0);
    assert_eq!(vp.scene.doc.guides.len(), 1);
}

#[test]
fn test_ctrl_turns_the_protractor_into_a_plain_protractor() {
    let mut vp = vp_with(None);
    let mut tool = ProtractorTool::new();
    assert!(tap(&mut tool, &mut vp));
    assert!(!tool.guides);
    measure(&mut vp, &mut tool, 45.0);
    assert!(
        vp.scene.doc.guides.is_empty(),
        "measure only: nothing left behind"
    );
    assert!(
        flashed(&mut vp).iter().any(|m| m.contains('°')),
        "…but it reported the angle"
    );
}

// The capitals stress the case the test is about.
#[test]
#[allow(non_snake_case)]
fn test_ctrl_still_means_COPY_on_rotate() {
    let mut vp = vp_with(None);
    let f = vp
        .scene
        .mesh_mut()
        .add_face(&[v(2., 0.), v(3., 0.), v(3., 1.), v(2., 1.)], &[]);
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    let mut tool = RotateTool::new();
    assert!(tap(&mut tool, &mut vp));
    assert!(tool.copy);
}

#[test]
fn test_ctrl_as_part_of_a_shortcut_leaves_the_mode_alone() {
    let mut vp = x_edge_vp();
    let mut tool = MeasureTool::new();
    let before = tool.mode;
    vp.key_press(&KeyEvent::new(Key::Control, "", Modifiers::CTRL));
    tool.on_key(&mut vp, Key::Control, Modifiers::NONE);
    assert_eq!(tool.mode, before); // the press alone: nothing yet
                                   // a Z came between (Ctrl+Z)
    vp.shortcut_override(&KeyEvent::new(Key::Char('Z'), "z", Modifiers::CTRL));
    assert_eq!(tool.on_key_release(&mut vp, Key::Control), Some(false));
    assert_eq!(tool.mode, before);
    // a clean tap
    vp.key_press(&KeyEvent::new(Key::Control, "", Modifiers::CTRL));
    tool.on_key(&mut vp, Key::Control, Modifiers::NONE);
    assert_eq!(tool.on_key_release(&mut vp, Key::Control), Some(true));
    assert_ne!(tool.mode, before);
}

#[test]
fn test_the_viewport_knows_a_ctrl_tap_from_ctrl_z() {
    let mut vp = viewport();
    vp.key_press(&KeyEvent::new(Key::Control, "", Modifiers::CTRL));
    assert!(vp.ctrl_tapped());
    vp.shortcut_override(&KeyEvent::new(Key::Char('Z'), "z", Modifiers::CTRL));
    assert!(!vp.ctrl_tapped()); // Ctrl+Z: not a tap
    vp.key_press(&KeyEvent::new(Key::Control, "", Modifiers::CTRL));
    assert!(vp.ctrl_tapped()); // a fresh press re-arms it
}

/// The width of `text` in the status bar's font (egui's body text).
fn text_width(ctx: &egui::Context, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(&ctx.global_style());
    ctx.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font, egui::Color32::WHITE)
            .size()
            .x
    })
}

/// The usual status bar keeps the modifiers on screen the whole time the
/// tool is active — «Ctrl = …» — instead of flashing them once. A flash
/// says what just happened; this says what you can do and which way it is
/// set.
#[test]
fn test_the_status_bar_keeps_the_ctrl_clause_up() {
    let mut win = window();
    win.activate_tool("measure");
    // the active mode is bracketed, and it MOVES with Ctrl — read off the
    // status bar itself, with no mouse movement in between
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(win.status_hint());
        win.viewport.with_tool(tap);
    }
    assert!(seen.iter().all(|t| t.contains("Ctrl =")));
    let distinct: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!(distinct.len(), 3, "the clause must change with the mode");
    let at = |t: &String| t.find('[').unwrap();
    assert!(at(&seen[0]) < at(&seen[1]) && at(&seen[1]) < at(&seen[2]));

    // ONE modifier at a time, each in the phase where it does something:
    // the usual line carries Ctrl before the first click and Alt once the
    // measurement is under way, never both. Stacked, the bar ran out of
    // room and elided.
    let tool = win.viewport.tool_mut::<MeasureTool>().unwrap();
    tool.start_point = Some(v3(1.0, 1.0, 0.0));
    let tool = win.viewport.active_tool();
    let midway = hint_for(Some("measure"), tool, None, Some(LinearMode::Off));
    assert!(!midway.contains("Ctrl ="));
    assert!(midway.contains("Alt ="));

    // …and it fits: the bar caps the message at its share of the width
    // (less the label's margins) on a 1366-wide window, in the shell's
    // font.
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
    let bar = 1366.0 - 2.0 * 8.0; // the bottom panel's inner margins
    let cap = StatusBar::message_max_width(bar) as f32 - 4.0;
    assert!(text_width(&ctx, "Ctrl =") > 20.0, "the font is loaded");
    for text in seen.iter().chain([&midway]) {
        assert!(text_width(&ctx, text) <= cap, "{text}");
    }
    // the glyphs just laid out are an upload nobody makes here
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
}
