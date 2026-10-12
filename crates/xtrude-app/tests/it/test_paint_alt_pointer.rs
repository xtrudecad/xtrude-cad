// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Alt is heard application wide — the Paint pointer follows it, a clean tap
//! toggles the eyedropper, Alt+Tab is not a tap. The shell hands every
//! key event to `Viewport::app_key_event`.

use crate::common;
use xtrude_app::viewport::Cursor;

use common::viewport;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::paint::PaintTool;
use xtrude_app::tools::{paint_sample_armed, set_paint_sample_armed};
use xtrude_app::viewport::Viewport;

fn paint_viewport() -> Viewport {
    let mut vp = viewport();
    vp.set_active_tool(Some(Box::new(PaintTool::new())));
    vp
}

fn alt(vp: &mut Viewport, press: bool) {
    let m = if press {
        Modifiers::ALT
    } else {
        Modifiers::NONE
    };
    vp.app_key_event(&KeyEvent::new(Key::Alt, "", m), press);
}

#[test]
fn test_alt_taps_on_the_menu_bar_still_reach_the_viewport() {
    set_paint_sample_armed(false);
    let mut vp = paint_viewport();
    alt(&mut vp, true);
    assert!(vp.alt_down());
    assert_eq!(
        vp.outbox.cursor,
        Some(Cursor::Tool {
            key: "eyedropper",
            plus: false
        })
    );
    alt(&mut vp, false);
    assert!(!vp.alt_down());
    set_paint_sample_armed(false);
}

#[test]
fn test_a_tap_toggles_the_eyedropper_and_it_stays() {
    set_paint_sample_armed(false);
    let mut vp = paint_viewport();
    alt(&mut vp, true);
    alt(&mut vp, false);
    assert!(paint_sample_armed());
    alt(&mut vp, true);
    alt(&mut vp, false);
    assert!(!paint_sample_armed());
    set_paint_sample_armed(false);
}

#[test]
fn test_alt_tab_is_not_a_tap() {
    set_paint_sample_armed(false);
    let mut vp = paint_viewport();
    alt(&mut vp, true);
    vp.app_key_event(&KeyEvent::new(Key::Tab, "", Modifiers::ALT), true);
    alt(&mut vp, false);
    assert!(!paint_sample_armed());
    set_paint_sample_armed(false);
}
