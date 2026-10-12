// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! One frame's hover moves reach the viewport as their last one; drags,
//! presses, releases, the wheel and keys all reach it, in order.

use xtrude_app::input::{coalesce_hovers, Button, Input, Key, KeyEvent, Modifiers, MouseEvent};

/// Each event as a short tag with its position, to compare sequences.
fn tags(inputs: &[Input]) -> Vec<String> {
    inputs
        .iter()
        .map(|ev| match ev {
            Input::Press(e) => format!("press {} {}", e.x, e.y),
            Input::DoubleClick(e) => format!("double {} {}", e.x, e.y),
            Input::Move(e) if e.left || e.middle || e.right => format!("drag {} {}", e.x, e.y),
            Input::Move(e) => format!("hover {} {}", e.x, e.y),
            Input::Release(e) => format!("release {} {}", e.x, e.y),
            Input::Wheel { x, y, .. } => format!("wheel {x} {y}"),
            Input::KeyPress(k) => format!("key {:?}", k.key),
            Input::KeyRelease(k) => format!("keyup {:?}", k.key),
        })
        .collect()
}

fn hover(x: f64) -> Input {
    Input::Move(MouseEvent::at(x, 0.))
}

fn drag(x: f64, b: Button) -> Input {
    Input::Move(MouseEvent::drag(x, 0., b))
}

#[test]
fn test_hover_moves_collapse_to_the_last() {
    let out = coalesce_hovers(vec![hover(1.), hover(2.), hover(3.)]);
    assert_eq!(tags(&out), ["hover 3 0"]);
    assert!(coalesce_hovers(Vec::new()).is_empty());
}

#[test]
fn test_moves_with_a_button_held_are_all_delivered() {
    for b in [Button::Left, Button::Middle, Button::Right] {
        let out = coalesce_hovers(vec![
            Input::Press(MouseEvent::button(0., 0., b, true)),
            drag(1., b),
            drag(2., b),
            drag(3., b),
            Input::Release(MouseEvent::button(3., 0., b, false)),
            hover(4.),
            hover(5.),
        ]);
        assert_eq!(
            tags(&out),
            [
                "press 0 0",
                "drag 1 0",
                "drag 2 0",
                "drag 3 0",
                "release 3 0",
                "hover 5 0"
            ]
        );
    }
}

#[test]
fn test_other_events_split_the_runs_of_hovers() {
    let key = KeyEvent::new(Key::Char('L'), "l", Modifiers::NONE);
    let out = coalesce_hovers(vec![
        hover(1.),
        hover(2.),
        Input::KeyPress(key.clone()),
        hover(3.),
        hover(4.),
        Input::Wheel {
            x: 4.,
            y: 0.,
            delta: 120.,
        },
        hover(5.),
        Input::Press(MouseEvent::button(5., 0., Button::Left, true)),
        Input::Release(MouseEvent::button(5., 0., Button::Left, false)),
        hover(6.),
        Input::KeyRelease(key),
        hover(7.),
        hover(8.),
    ]);
    assert_eq!(
        tags(&out),
        [
            "hover 2 0",
            "key Char('L')",
            "hover 4 0",
            "wheel 4 0",
            "hover 5 0",
            "press 5 0",
            "release 5 0",
            "hover 6 0",
            "keyup Char('L')",
            "hover 8 0"
        ]
    );
}
