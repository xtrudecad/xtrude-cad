// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The mouse navigation styles: which button, chord or modifier moves
//! the camera, as a few named presets (Preferences ▸ Navigation ▸ Mouse
//! navigation).
//!
//! Every style zooms with the wheel, and the camera modes (Orbit, Pan,
//! Zoom, Zoom Window) take the left button in every style. A style only
//! says what the other drags do: a [`NavStyle`] is a short table of
//! [`Binding`]s, read first to last, the first match winning.

use xtrude_model::i18n::tr;

use crate::input::{Button, Modifiers};

/// What a navigation drag does to the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavAction {
    Orbit,
    Pan,
    /// Up zooms in, down zooms out.
    Zoom,
}

/// What starts a navigation drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// A drag with this button.
    Drag(Button),
    /// The left and the right button held together, in either order.
    LeftRightChord,
    /// A pointer move with no button held (trackpads).
    Move,
}

/// One row of a style: a trigger and the modifiers it takes.
///
/// `shift` must match exactly; `ctrl` and `alt`, when set, must be held
/// and are ignored otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub trigger: Trigger,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub action: NavAction,
}

impl Binding {
    const fn new(trigger: Trigger, action: NavAction) -> Binding {
        Binding {
            trigger,
            shift: false,
            ctrl: false,
            alt: false,
            action,
        }
    }

    const fn shift(mut self) -> Binding {
        self.shift = true;
        self
    }

    const fn ctrl(mut self) -> Binding {
        self.ctrl = true;
        self
    }

    const fn alt(mut self) -> Binding {
        self.alt = true;
        self
    }

    fn matches(&self, trigger: Trigger, m: Modifiers) -> bool {
        self.trigger == trigger
            && self.shift == m.shift
            && (!self.ctrl || m.ctrl)
            && (!self.alt || m.alt)
    }

    /// The modifiers as the bindings line spells them, `""` for none.
    fn keys(&self) -> String {
        let mut keys = Vec::new();
        if self.ctrl {
            keys.push(tr("Ctrl", &[]));
        }
        if self.alt {
            keys.push(tr("Alt", &[]));
        }
        if self.shift {
            keys.push(tr("Shift", &[]));
        }
        keys.join(" + ")
    }

    /// The binding in a bindings line: «Shift + middle-drag», «right-drag».
    pub fn describe(&self) -> String {
        let what = match self.trigger {
            Trigger::Drag(b) => tr("{button}-drag", &[("button", &button_name(b))]),
            Trigger::LeftRightChord => tr("left + right drag", &[]),
            Trigger::Move => tr("move", &[]),
        };
        let keys = self.keys();
        if keys.is_empty() {
            what
        } else {
            format!("{keys} + {what}")
        }
    }
}

fn button_name(b: Button) -> String {
    tr(
        match b {
            Button::Left => "left",
            Button::Middle => "middle",
            Button::Right => "right",
        },
        &[],
    )
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

use NavAction::{Orbit, Pan, Zoom};
use Trigger::{Drag, LeftRightChord, Move};

const MIDDLE: Trigger = Drag(Button::Middle);
const RIGHT: Trigger = Drag(Button::Right);

const DEFAULT: &[Binding] = &[
    Binding::new(MIDDLE, Pan).shift(),
    Binding::new(MIDDLE, Orbit),
];
const MIDDLE_PANS: &[Binding] = &[
    Binding::new(MIDDLE, Orbit).shift(),
    Binding::new(MIDDLE, Pan),
];
const CHORD: &[Binding] = &[
    Binding::new(MIDDLE, Orbit),
    Binding::new(LeftRightChord, Pan),
    Binding::new(MIDDLE, Pan).shift(),
];
const RIGHT_BUTTON: &[Binding] = &[
    Binding::new(RIGHT, Orbit),
    Binding::new(RIGHT, Pan).shift(),
    Binding::new(MIDDLE, Orbit),
    Binding::new(MIDDLE, Pan).shift(),
];
// Everything under Alt, so Shift (the inference lock) and Ctrl keep
// their meaning while drawing. Zoom comes first: the orbit row, which
// ignores Ctrl, would take Ctrl + Alt too.
const TOUCHPAD: &[Binding] = &[
    Binding::new(Move, Zoom).alt().ctrl(),
    Binding::new(Move, Orbit).alt(),
    Binding::new(Move, Pan).alt().shift(),
    Binding::new(MIDDLE, Orbit),
    Binding::new(MIDDLE, Pan).shift(),
];

/// A named set of mouse bindings for the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavStyle {
    /// Middle-drag orbits, Shift + middle-drag pans.
    #[default]
    Default,
    /// Middle-drag pans, Shift + middle-drag orbits.
    MiddlePans,
    /// Middle-drag orbits; the left and right buttons held together pan.
    Chord,
    /// Right-drag orbits, Shift + right-drag pans; a right click still
    /// opens the context menu. For a mouse without a middle button.
    RightButton,
    /// No button needed: Alt + move orbits, Alt + Shift + move pans,
    /// Ctrl + Alt + move zooms.
    Touchpad,
}

impl NavStyle {
    pub const ALL: [NavStyle; 5] = [
        NavStyle::Default,
        NavStyle::MiddlePans,
        NavStyle::Chord,
        NavStyle::RightButton,
        NavStyle::Touchpad,
    ];

    /// The stored name (`nav/style`).
    pub fn key(self) -> &'static str {
        match self {
            NavStyle::Default => "default",
            NavStyle::MiddlePans => "middle_pans",
            NavStyle::Chord => "chord",
            NavStyle::RightButton => "right_button",
            NavStyle::Touchpad => "touchpad",
        }
    }

    /// The style a stored name gives; an unknown one is the default.
    pub fn from_key(key: &str) -> NavStyle {
        NavStyle::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }

    /// The name Preferences shows (untranslated).
    pub fn label(self) -> &'static str {
        match self {
            NavStyle::Default => "Default",
            NavStyle::MiddlePans => "Middle button pans",
            NavStyle::Chord => "Two-button chord",
            NavStyle::RightButton => "Right button",
            NavStyle::Touchpad => "Touchpad",
        }
    }

    pub fn bindings(self) -> &'static [Binding] {
        match self {
            NavStyle::Default => DEFAULT,
            NavStyle::MiddlePans => MIDDLE_PANS,
            NavStyle::Chord => CHORD,
            NavStyle::RightButton => RIGHT_BUTTON,
            NavStyle::Touchpad => TOUCHPAD,
        }
    }

    /// What `trigger` with `m` held does, if anything.
    pub fn action(self, trigger: Trigger, m: Modifiers) -> Option<NavAction> {
        self.bindings()
            .iter()
            .find(|b| b.matches(trigger, m))
            .map(|b| b.action)
    }

    /// Whether a drag with `button` moves the camera with some modifiers.
    pub fn drags_with(self, button: Button) -> bool {
        self.bindings().iter().any(|b| {
            b.trigger == Drag(button) || (b.trigger == LeftRightChord && button != Button::Middle)
        })
    }

    /// The bindings of `action`, as one line's items: «right-drag»,
    /// «Shift + middle-drag».
    fn describe(self, action: NavAction) -> Vec<String> {
        self.bindings()
            .iter()
            .filter(|b| b.action == action)
            .map(Binding::describe)
            .collect()
    }

    /// The cheat sheet Preferences shows: one `(what, how)` row each for
    /// orbit, pan, zoom and the context menu.
    pub fn cheat_sheet(self) -> Vec<(String, String)> {
        let or = tr(", or ", &[]);
        let mut zoom = vec![tr("Wheel", &[])];
        zoom.extend(self.describe(Zoom));
        let menu = if self.drags_with(Button::Right) {
            tr("Right-click without dragging", &[])
        } else {
            tr("Right-click", &[])
        };
        vec![
            (
                tr("Orbit", &[]),
                capitalise(&self.describe(Orbit).join(&or)),
            ),
            (tr("Pan", &[]), capitalise(&self.describe(Pan).join(&or))),
            (tr("Zoom", &[]), zoom.join(&or)),
            (tr("Context menu", &[]), menu),
        ]
    }

    /// The Orbit mode's reminder that the style orbits from any tool:
    /// «The middle button orbits from any tool.»
    pub fn orbit_reminder(self) -> String {
        let Some(b) = self.bindings().iter().find(|b| b.action == Orbit) else {
            return String::new();
        };
        let keys = b.keys();
        match (b.trigger, keys.is_empty()) {
            (Drag(button), true) => tr(
                "The {button} button orbits from any tool.",
                &[("button", &button_name(button))],
            ),
            (Drag(button), false) => tr(
                "{keys} + the {button} button orbits from any tool.",
                &[("keys", &keys), ("button", &button_name(button))],
            ),
            (Move, false) => tr("{keys} + a move orbits from any tool.", &[("keys", &keys)]),
            _ => tr(
                "{how} orbits from any tool.",
                &[("how", &capitalise(&b.describe()))],
            ),
        }
    }
}
