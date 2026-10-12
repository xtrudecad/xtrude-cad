// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Activating a tool turns the pointer into the tool's icon (aim at the
//! hotspot); Select keeps the arrow. The viewport names its pointer
//! (`Viewport::cursor`); the shell makes it the OS cursor.

use crate::common;
use xtrude_app::viewport::Cursor;

use xtrude_app::shell::icons::{self, Rgba};

const INK: Rgba = Rgba(224, 224, 224, 255);

fn hotspot(key: &str) -> Option<(f32, f32)> {
    icons::cursor(key, false, INK, 1.0).map(|(_, h)| h)
}

#[test]
fn test_tool_cursor_builds_bitmap_with_hotspot() {
    let (img, hs) = icons::cursor("line", false, INK, 1.0).expect("pencil");
    assert!(img.rgba.iter().any(|&b| b != 0));
    assert_eq!(hs, (4.0, 28.0)); // (6,42) in 48-space → ×32/48
    assert_eq!(hotspot("eraser"), Some((10.0, 21.0)));
    assert_eq!(hotspot("move"), Some((16.0, 16.0)));
    assert!(icons::cursor("select", false, INK, 1.0).is_none());
    assert!(icons::cursor("no_such_tool", false, INK, 1.0).is_none());
    // (The shell caches the texture.)
    assert_eq!(hotspot("orbit"), Some((16.0, 16.0)));
    assert!(hotspot("pan").is_some());
    assert_eq!(hotspot("zoom"), Some((14.0, 14.0))); // lens centre
    assert!(hotspot("zoom_window").is_some());
}

#[test]
fn test_activating_tools_swaps_the_viewport_cursor() {
    let mut win = common::window();
    let drawn = |w: &xtrude_app::window::MainWindow| {
        w.viewport
            .outbox
            .cursor
            .and_then(|c: Cursor| icons::cursor(c.key(), false, INK, 1.0))
            .is_some()
    };
    assert!(!drawn(&win)); // select at startup
    win.activate_tool("line");
    assert!(drawn(&win));
    win.activate_tool("pushpull");
    assert!(drawn(&win));
    win.activate_tool("select");
    assert!(!drawn(&win)); // back to the arrow
}

#[test]
fn test_select_shows_the_system_arrow() {
    use xtrude_app::shell::{viewport_pointer, ViewportPointer};
    let mut win = common::window();
    let arrow = ViewportPointer::Icon(egui::CursorIcon::Default);
    assert_eq!(win.viewport.outbox.cursor, None); // select at startup
    assert_eq!(viewport_pointer(win.viewport.outbox.cursor), arrow);
    win.activate_tool("line");
    assert_ne!(viewport_pointer(win.viewport.outbox.cursor), arrow);
    win.activate_tool("select");
    assert_eq!(viewport_pointer(win.viewport.outbox.cursor), arrow);
}

#[test]
fn test_os_cursor_is_in_physical_pixels() {
    for dpr in [1.0f32, 2.0] {
        let c = icons::os_cursor("line", false, INK, dpr).expect("pencil");
        let side = (icons::CURSOR_SIZE * dpr) as u16;
        assert_eq!(c.size, [side, side]);
        assert_eq!(c.rgba.len(), side as usize * side as usize * 4);
        let hot = [(4.0 * dpr) as u16, (28.0 * dpr) as u16];
        assert_eq!(c.hotspot, hot); // (4, 28) at 1, (8, 56) at 2
    }
    assert!(icons::os_cursor("select", false, INK, 1.0).is_none());
}

#[test]
fn test_os_cursor_has_straight_alpha() {
    // the OS takes straight alpha; the icons are premultiplied
    assert_eq!(
        icons::unpremultiply(&[64, 32, 0, 128, 9, 9, 9, 0, 200, 100, 50, 255]),
        vec![128, 64, 0, 128, 0, 0, 0, 0, 200, 100, 50, 255]
    );
    let (img, _) = icons::cursor("line", false, INK, 1.0).expect("pencil");
    let c = icons::os_cursor("line", false, INK, 1.0).expect("pencil");
    assert_eq!(c.rgba, icons::unpremultiply(&img.rgba));
}

#[test]
fn test_cursor_texts_are_in_the_pixels() {
    // the OS shows the cursor, so nothing is left for the shell to draw
    for key in ["dimension", "text3d", "solid_1", "solid_2", "line"] {
        let (img, _) = icons::cursor(key, false, INK, 1.0).expect(key);
        assert!(img.texts.is_empty(), "{key}");
    }
    // with a system font, the "1" and "2" differ inside the solid badge
    if xtrude_core::text3d::vertical_metrics("sans-serif", true, false).is_some() {
        let one = icons::cursor("solid_1", false, INK, 1.0).expect("1").0.rgba;
        let two = icons::cursor("solid_2", false, INK, 1.0).expect("2").0.rgba;
        assert_ne!(one, two);
    }
}

#[test]
fn test_has_cursor_matches_the_bitmaps() {
    let keys = [
        "select",
        "line",
        "rectangle",
        "arc",
        "move",
        "pushpull",
        "dimension",
        "text3d",
        "paint",
        "eraser",
        "measure",
        "orbit",
        "pan",
        "zoom",
        "zoom_window",
        "section",
        "solid_no",
        "solid_1",
        "solid_2",
        "solid_3",
        "no_such_tool",
    ];
    for key in keys {
        assert_eq!(
            icons::has_cursor(key),
            icons::cursor(key, false, INK, 1.0).is_some(),
            "{key}"
        );
    }
}

#[test]
fn test_viewport_pointer_choice() {
    use xtrude_app::shell::{viewport_pointer, ViewportPointer};
    let line = Cursor::Tool {
        key: "line",
        plus: false,
    };
    assert_eq!(viewport_pointer(Some(line)), ViewportPointer::Image(line));
    // Select keeps the system arrow
    assert_eq!(
        viewport_pointer(None),
        ViewportPointer::Icon(egui::CursorIcon::Default)
    );
    let select = Cursor::Tool {
        key: "select",
        plus: false,
    };
    assert_eq!(
        viewport_pointer(Some(select)),
        ViewportPointer::Icon(egui::CursorIcon::Crosshair)
    );
    // the navigation pointers are bitmaps too, as they were drawn before
    for c in [Cursor::Orbit, Cursor::Pan, Cursor::Zoom, Cursor::ZoomWindow] {
        assert_eq!(viewport_pointer(Some(c)), ViewportPointer::Image(c));
    }
}
