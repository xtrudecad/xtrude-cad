// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Toolkit-neutral input events: what the viewport and the tools react to.
//! The egui front-end translates its events into these; tests build them
//! directly.

/// Keyboard modifiers held during an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    };
    pub const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
        meta: false,
    };

    pub fn with(self, other: Modifiers) -> Modifiers {
        Modifiers {
            shift: self.shift || other.shift,
            ctrl: self.ctrl || other.ctrl,
            alt: self.alt || other.alt,
            meta: self.meta || other.meta,
        }
    }

    pub fn is_empty(self) -> bool {
        !(self.shift || self.ctrl || self.alt || self.meta)
    }
}

/// Mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// The keys the viewport and tools act on (plus `Char` for text keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Return,
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    Left,
    Right,
    Up,
    Down,
    Shift,
    Control,
    Alt,
    Meta,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    /// A key that types a character (letters, digits, punctuation),
    /// identified by its upper-case character.
    Char(char),
}

/// A key press or release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    /// What the key types with the current modifiers ("" for none).
    pub text: String,
    pub modifiers: Modifiers,
    pub auto_repeat: bool,
}

impl KeyEvent {
    pub fn new(key: Key, text: &str, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            text: text.to_string(),
            modifiers,
            auto_repeat: false,
        }
    }

    /// A key that types `ch` (digits, letters, marks).
    pub fn typed(ch: char) -> KeyEvent {
        let key = match ch {
            ' ' => Key::Space,
            c => Key::Char(c.to_ascii_uppercase()),
        };
        KeyEvent::new(key, &ch.to_string(), Modifiers::NONE)
    }
}

/// A position in the viewport, in logical pixels from its top left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenPt {
    pub x: f64,
    pub y: f64,
}

impl ScreenPt {
    pub const fn new(x: f64, y: f64) -> ScreenPt {
        ScreenPt { x, y }
    }

    pub fn distance(self, other: ScreenPt) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// `(x, y)`, for the overlay's paint commands and the snapping maths.
    pub fn tuple(self) -> (f64, f64) {
        (self.x, self.y)
    }
}

impl From<(f64, f64)> for ScreenPt {
    fn from((x, y): (f64, f64)) -> ScreenPt {
        ScreenPt { x, y }
    }
}

/// A pointer event at `(x, y)` logical pixels from the viewport's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    /// The button that changed (press / release), if any.
    pub button: Option<Button>,
    pub left: bool,
    pub middle: bool,
    pub right: bool,
    pub modifiers: Modifiers,
    /// When it happened (ms, any monotonic origin): the triple click.
    pub time_ms: u64,
}

impl MouseEvent {
    pub fn at(x: f64, y: f64) -> MouseEvent {
        MouseEvent {
            x,
            y,
            button: None,
            left: false,
            middle: false,
            right: false,
            modifiers: Modifiers::NONE,
            time_ms: 0,
        }
    }

    pub fn at_time(mut self, ms: u64) -> MouseEvent {
        self.time_ms = ms;
        self
    }

    /// A press or release of `button` (held during a move: `held`).
    pub fn button(x: f64, y: f64, button: Button, held: bool) -> MouseEvent {
        let mut e = MouseEvent::at(x, y);
        e.button = Some(button);
        e.set_held(button, held);
        e
    }

    /// A move with `button` held.
    pub fn drag(x: f64, y: f64, button: Button) -> MouseEvent {
        let mut e = MouseEvent::at(x, y);
        e.set_held(button, true);
        e
    }

    pub fn with_modifiers(mut self, m: Modifiers) -> MouseEvent {
        self.modifiers = m;
        self
    }

    fn set_held(&mut self, button: Button, held: bool) {
        match button {
            Button::Left => self.left = held,
            Button::Middle => self.middle = held,
            Button::Right => self.right = held,
        }
    }
}

/// One input event for the viewport, as the front-end hands them over.
#[derive(Clone, Debug)]
pub enum Input {
    Press(MouseEvent),
    DoubleClick(MouseEvent),
    Move(MouseEvent),
    Release(MouseEvent),
    Wheel { x: f64, y: f64, delta: f64 },
    KeyPress(KeyEvent),
    KeyRelease(KeyEvent),
}

impl Input {
    /// A pointer move with no button held: only a hover.
    fn is_hover(&self) -> bool {
        matches!(self, Input::Move(e) if !(e.left || e.middle || e.right))
    }
}

/// One frame's events with each run of hover moves (moves with no button
/// held, nothing else in between) reduced to its last move, so the hover
/// work of a frame does not grow with the number of pointer reports.
/// Every other event, and every move with a button held, is kept in order:
/// drags and the tools that draw see each one.
pub fn coalesce_hovers(inputs: Vec<Input>) -> Vec<Input> {
    let mut out: Vec<Input> = Vec::with_capacity(inputs.len());
    for ev in inputs {
        if ev.is_hover() && out.last().is_some_and(Input::is_hover) {
            out.pop();
        }
        out.push(ev);
    }
    out
}
