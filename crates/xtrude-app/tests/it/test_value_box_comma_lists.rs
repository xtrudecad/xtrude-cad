// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! "200,100" is two values in the Rectangle, a decimal everywhere else.

use crate::common;

use common::{close, window, MetreDocument};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::rectangle::RectangleTool;
use xtrude_app::tools::Tool;
use xtrude_app::value_box::{parse_value_buffer, Typed};

fn same(got: Option<Typed>, want: &[f64]) -> bool {
    let vals = match got {
        Some(Typed::Value(a)) => vec![a],
        Some(Typed::Pair(a, b)) => vec![a, b],
        Some(Typed::Triple(a, b, c)) => vec![a, b, c],
        _ => return false,
    };
    vals.len() == want.len() && vals.iter().zip(want).all(|(a, b)| close(*a, *b))
}

#[test]
fn test_a_multi_value_tool_reads_the_comma_as_a_list_separator() {
    let _m = MetreDocument::new();
    assert!(same(parse_value_buffer("200,100", true), &[200.0, 100.0]));
    assert!(same(parse_value_buffer("3,2,1", true), &[3.0, 2.0, 1.0]));
    assert!(same(parse_value_buffer("2,5;1,2", true), &[2.5, 1.2]));
    assert!(same(parse_value_buffer("200, 100", true), &[200.0, 100.0]));
    assert!(same(parse_value_buffer("200;100", true), &[200.0, 100.0]));
}

#[test]
fn test_everywhere_else_the_comma_is_still_the_decimal() {
    let _m = MetreDocument::new();
    assert!(same(parse_value_buffer("2,5", false), &[2.5]));
    assert!(same(parse_value_buffer("200,100", false), &[200.1]));
    assert!(same(parse_value_buffer("2,5;1,2", false), &[2.5, 1.2]));
    assert!(same(parse_value_buffer("200 100", false), &[200.0, 100.0]));
    assert!(same(parse_value_buffer("200, 100", false), &[200.0, 100.0]));
}

#[test]
fn test_typing_200_comma_100_in_the_rectangle_draws_200_by_100() {
    let _m = MetreDocument::new();
    let mut w = window();
    w.activate_tool("rectangle");
    let vp = &mut w.viewport;
    {
        let t = vp.tool_mut::<RectangleTool>().unwrap();
        assert!(t.value_spec().comma_lists);
        t.start_point = Some(DVec3::ZERO);
        t.hover_point = Some(DVec3::new(1., 1., 0.)); // dragging toward +x +y
    }
    for ch in "200,100".chars() {
        w.key_press(&KeyEvent::typed(ch));
    }
    w.key_press(&KeyEvent::new(Key::Return, "", Modifiers::NONE));
    // The rectangle it drew says what it was given.
    let m = w.viewport.scene.mesh();
    let f = *m.face_ids().first().expect("Enter did not reach the tool");
    let ps = m.face_positions(f);
    let span = |k: fn(&DVec3) -> f64| {
        let lo = ps.iter().map(k).fold(f64::MAX, f64::min);
        let hi = ps.iter().map(k).fold(f64::MIN, f64::max);
        hi - lo
    };
    assert!(close(span(|p| p.x), 200.0) && close(span(|p| p.y), 100.0));
}
