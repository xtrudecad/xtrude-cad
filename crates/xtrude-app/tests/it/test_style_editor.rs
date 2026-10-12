// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Panel tests: the Styles panel picks a preset, edits the active style live,
//! keeps every swatch editable with a hint when it cannot show, lists saved
//! styles and arms Delete for them; the Back swatch shows the tint that draws;
//! the sky gradient ramps from haze to the picked tones. (The style library
//! tests are in xtrude-model.)

use crate::common;

use common::window;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::tray::styles::Styles;
use xtrude_app::viewport::render::{sky_gradient, SkyGradient};
use xtrude_app::window::{MainWindow, ScriptedDialogs};
use xtrude_model::settings;
use xtrude_model::style::{save_user_style, Style};

fn setup() -> (MainWindow, Styles) {
    let w = window();
    settings::remove("styles/user");
    let mut p = Styles::default();
    p.refresh(&w);
    (w, p)
}

#[test]
fn test_panel_pick_applies_a_preset() {
    let (mut w, mut p) = setup();
    let i = p.find("Hidden line").expect("listed");
    p.pick(&mut w, i);
    assert_eq!(w.viewport.scene.display_style.face_mode, "hidden_line");
    assert!(!w.viewport.scene.display_style.sky);
}

#[test]
fn test_panel_edits_the_active_style_live() {
    let (mut w, mut p) = setup();
    p.edit(&mut w, |s| s.edges = false);
    p.edit(&mut w, |s| s.face_mode = "xray".into());
    let style = &w.viewport.scene.display_style;
    assert!(!style.edges);
    assert_eq!(style.face_mode, "xray");
}

#[test]
fn test_every_swatch_stays_editable_and_hints_when_invisible() {
    let (mut w, p) = setup();
    // The default style: shaded, sky, edges, section fill.
    assert!(p.color_hint(&w, "front_color").is_some()); // can't show shaded
    assert!(p.color_hint(&w, "background").is_some()); // the sky covers it
    assert!(p.color_hint(&w, "sky_color").is_none());
    assert!(p.color_hint(&w, "edge_color").is_none());
    let s = &mut w.viewport.scene.display_style;
    s.face_mode = "hidden_line".into();
    s.sky = false;
    s.edges = false;
    s.section_fill = false;
    assert!(p.color_hint(&w, "front_color").is_none());
    assert!(p.color_hint(&w, "background").is_none());
    assert!(p.color_hint(&w, "sky_color").is_some()); // the sky is off now
    assert!(p.color_hint(&w, "edge_color").is_some()); // edges off
    assert!(p.color_hint(&w, "section_fill_color").is_some());
    w.viewport.scene.display_style.face_mode = "wireframe".into();
    assert!(p.color_hint(&w, "edge_color").is_none()); // wireframe always draws
}

#[test]
fn test_sky_gradient_ramps_from_haze_to_the_picked_tones() {
    let (sky, gnd) = ([0.2, 0.4, 0.9], [0.4, 0.3, 0.2]);
    let SkyGradient {
        sky_bottom: s0,
        sky_top: s1,
        ground_top: g0,
        ground_bottom: g1,
        horizon_line: line,
    } = sky_gradient(sky, gnd, 0.0);
    assert!(s0[0] > sky[0] && g0[0] > gnd[0]); // hazed at the horizon
    assert!(s1[0] < s0[0] && g1[0] < g0[0]); // the picked tones away from it
    for c in [s0, s1, g0, g1, line] {
        assert!(c.iter().all(|v| (0.0..=1.0).contains(v)));
    }
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
    let SkyGradient {
        sky_bottom: s0,
        sky_top: s1,
        ..
    } = sky_gradient(sky, gnd, -3.0); // pitched up
    assert!(close(s0, sky) && close(s1, sky));
    let SkyGradient {
        ground_top: g0,
        ground_bottom: g1,
        ..
    } = sky_gradient(sky, gnd, 3.0); // pitched down
    assert!(close(g0, gnd) && close(g1, gnd));
}

#[test]
fn test_panel_combo_lists_saved_styles_and_arms_delete() {
    let (mut w, mut p) = setup();
    save_user_style(&Style {
        name: "Build".into(),
        face_mode: "hidden_line".into(),
        ..Style::default()
    })
    .unwrap();
    p.refresh(&w);
    let i = p.find("Build").expect("listed");
    p.pick(&mut w, i);
    assert_eq!(w.viewport.scene.display_style.name, "Build");
    assert!(p.delete_enabled); // a user style: deletable
    let d = p.find("Default").unwrap();
    p.pick(&mut w, d);
    assert!(!p.delete_enabled); // a built-in: not
}

#[test]
fn test_panel_back_swatch_shows_the_tint_that_draws() {
    let (mut w, mut p) = setup();
    let shown = Styles::shown_color(&w, "back_color").map(|c| (c * 255.0).round_ties_even() as i64);
    assert_eq!(shown, [158, 178, 199]); // automatic: the default blue-grey
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().colors.push(Some([1.0, 0.0, 0.0]));
    w.dialogs = Box::new(dialogs);
    p.pick_color(&mut w, "Back color", "back_color");
    let style = &w.viewport.scene.display_style;
    assert_eq!(style.back_color, Some([1.0, 0.0, 0.0]));
    assert_eq!(Styles::shown_color(&w, "back_color"), [1.0, 0.0, 0.0]);
    assert_eq!(style.front_color, [1.0, 1.0, 1.0]); // front untouched
}

#[test]
fn test_back_color_hint_only_where_back_faces_cannot_show() {
    let (mut w, p) = setup();
    for mode in ["shaded", "monochrome", "xray"] {
        w.viewport.scene.display_style.face_mode = mode.into();
        assert!(p.color_hint(&w, "back_color").is_none(), "{mode}");
    }
    for mode in ["hidden_line", "wireframe"] {
        w.viewport.scene.display_style.face_mode = mode.into();
        assert!(p.color_hint(&w, "back_color").is_some(), "{mode}");
    }
}
