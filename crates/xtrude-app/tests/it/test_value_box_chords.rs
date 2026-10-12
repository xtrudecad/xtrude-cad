// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Alt+1 with a value being typed stays a shortcut instead of appending a «1»:
//! the value box answers whether it takes the key. The NaN-file test
//! lives in xtrude-io (test_non_finite_values).

use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::value_box::{is_chord, parse_number, parse_value_buffer, ValueBox};

fn digit(ch: char, m: Modifiers) -> KeyEvent {
    KeyEvent::new(Key::Char(ch), &ch.to_string(), m)
}

fn takes(vb: &ValueBox, ev: &KeyEvent) -> bool {
    // The override question: would the value box take it? (on a copy)
    vb.clone().handle_key(ev, Default::default())
}

#[test]
fn test_a_digit_continues_the_value_but_alt_digit_stays_a_shortcut() {
    let vb = ValueBox {
        buffer: "12".into(),
    };
    assert!(takes(&vb, &digit('1', Modifiers::NONE)));
    assert!(!takes(&vb, &digit('1', Modifiers::ALT)));
    assert!(!takes(&vb, &digit('2', Modifiers::CTRL)));
    assert!(!takes(
        &vb,
        &KeyEvent::new(Key::Char('F'), "f", Modifiers::ALT)
    ));
}

#[test]
fn test_alt_digit_never_lands_in_the_buffer() {
    let mut vb = ValueBox {
        buffer: "12".into(),
    };
    vb.handle_key(&digit('1', Modifiers::ALT), Default::default());
    assert_eq!(vb.buffer, "12");
    vb.handle_key(&digit('3', Modifiers::NONE), Default::default());
    assert_eq!(vb.buffer, "123");
}

#[test]
fn test_altgr_still_types() {
    let vb = ValueBox {
        buffer: "12".into(),
    };
    let both = Modifiers::CTRL.with(Modifiers::ALT);
    assert!(!is_chord(both));
    assert!(takes(&vb, &digit('1', both)));
}

#[test]
fn test_an_absurd_number_is_refused() {
    assert_eq!(parse_number("111111111111111111111112111"), None);
    assert_eq!(parse_number("8900000"), Some(8900000.0));
    assert_eq!(
        parse_value_buffer("111111111111111111111112111", false),
        None
    );
}
