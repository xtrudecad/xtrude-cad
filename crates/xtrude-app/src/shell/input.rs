// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! egui's input as the window's:
//! pointer presses, moves, releases and double clicks inside the viewport,
//! the wheel, keys — modifiers alone included, which egui reports only as
//! state — and typed text.

pub use crate::input::Input;
use crate::input::{Button, Key, KeyEvent, Modifiers, MouseEvent};

/// The pointer and keyboard state carried between frames.
#[derive(Default)]
pub struct InputState {
    pub held: [bool; 3],
    pub modifiers: Modifiers,
    last_press: Option<(u64, (f64, f64), Button)>,
}

pub fn modifiers(m: egui::Modifiers) -> Modifiers {
    Modifiers {
        shift: m.shift,
        ctrl: m.ctrl || (cfg!(target_os = "macos") && m.mac_cmd),
        alt: m.alt,
        meta: false,
    }
}

fn button(b: egui::PointerButton) -> Option<Button> {
    match b {
        egui::PointerButton::Primary => Some(Button::Left),
        egui::PointerButton::Middle => Some(Button::Middle),
        egui::PointerButton::Secondary => Some(Button::Right),
        _ => None,
    }
}

/// egui's key as the window's, and the text it types (unshifted).
pub fn key(k: egui::Key) -> Option<(Key, String)> {
    use egui::Key as E;
    let named = |key: Key| Some((key, String::new()));
    match k {
        E::Enter => named(Key::Return),
        E::Escape => named(Key::Escape),
        E::Backspace => named(Key::Backspace),
        E::Delete => named(Key::Delete),
        E::Tab => named(Key::Tab),
        E::Space => Some((Key::Space, " ".into())),
        E::ArrowLeft => named(Key::Left),
        E::ArrowRight => named(Key::Right),
        E::ArrowUp => named(Key::Up),
        E::ArrowDown => named(Key::Down),
        E::Home => named(Key::Home),
        E::End => named(Key::End),
        E::PageUp => named(Key::PageUp),
        E::PageDown => named(Key::PageDown),
        E::F1 => named(Key::F(1)),
        E::F2 => named(Key::F(2)),
        E::F3 => named(Key::F(3)),
        E::F4 => named(Key::F(4)),
        E::F5 => named(Key::F(5)),
        E::F6 => named(Key::F(6)),
        E::F7 => named(Key::F(7)),
        E::F8 => named(Key::F(8)),
        E::F9 => named(Key::F(9)),
        E::F10 => named(Key::F(10)),
        E::F11 => named(Key::F(11)),
        E::F12 => named(Key::F(12)),
        _ => {
            let name = k.name();
            let mut chars = name.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                // punctuation keys egui names by their symbol, e.g. "-"
                let sym = k.symbol_or_name();
                let mut sc = sym.chars();
                let s = sc.next()?;
                if sc.next().is_some() {
                    return None;
                }
                return Some((Key::Char(s.to_ascii_uppercase()), s.to_string()));
            }
            Some((
                Key::Char(c.to_ascii_uppercase()),
                c.to_ascii_lowercase().to_string(),
            ))
        }
    }
}

const DOUBLE_CLICK_MS: u64 = 400;

/// One frame of egui input.
pub struct FrameInput<'a> {
    pub events: &'a [egui::Event],
    pub mods: egui::Modifiers,
    pub time_ms: u64,
    pub hover_pos: Option<egui::Pos2>,
}

impl InputState {
    /// Translate one frame of egui events; `origin` is the viewport's top
    /// left on screen, `inside` whether the pointer is over it.
    pub fn translate(
        &mut self,
        frame: &FrameInput<'_>,
        origin: egui::Pos2,
        inside: &dyn Fn(egui::Pos2) -> bool,
        keyboard_to_viewport: bool,
    ) -> Vec<Input> {
        let &FrameInput {
            events,
            mods,
            time_ms,
            hover_pos,
        } = frame;
        let mut out = Vec::new();
        let m = modifiers(mods);
        // Modifier keys alone: egui reports state, the tools want events.
        for (was, now, k) in [
            (self.modifiers.shift, m.shift, Key::Shift),
            (self.modifiers.ctrl, m.ctrl, Key::Control),
            (self.modifiers.alt, m.alt, Key::Alt),
        ] {
            if was != now && keyboard_to_viewport {
                let ev = KeyEvent::new(k, "", m);
                out.push(if now {
                    Input::KeyPress(ev)
                } else {
                    Input::KeyRelease(ev)
                });
            }
        }
        self.modifiers = m;
        let local = |p: egui::Pos2| ((p.x - origin.x) as f64, (p.y - origin.y) as f64);
        let held = |s: &InputState| (s.held[0], s.held[1], s.held[2]);
        let mouse = |s: &InputState, p: egui::Pos2| {
            let (x, y) = local(p);
            let (l, mi, r) = held(s);
            let mut e = MouseEvent::at(x, y).with_modifiers(m).at_time(time_ms);
            e.left = l;
            e.middle = mi;
            e.right = r;
            e
        };
        let mut typed_this_frame = false;
        for ev in events {
            match ev {
                egui::Event::PointerButton {
                    pos,
                    button: b,
                    pressed,
                    modifiers: em,
                } => {
                    let Some(b) = button(*b) else { continue };
                    let i = b as usize;
                    if *pressed && !inside(*pos) {
                        continue;
                    }
                    if !*pressed && !self.held[i] {
                        continue;
                    }
                    self.held[i] = *pressed;
                    let mut e = mouse(self, *pos);
                    e.button = Some(b);
                    e.modifiers = modifiers(*em);
                    if *pressed {
                        let (x, y) = (e.x, e.y);
                        let double = self.last_press.is_some_and(|(t, p, lb)| {
                            lb == b
                                && time_ms.saturating_sub(t) <= DOUBLE_CLICK_MS
                                && ((x - p.0).abs() + (y - p.1).abs()) < 8.0
                        });
                        if double {
                            self.last_press = None;
                            out.push(Input::DoubleClick(e));
                        } else {
                            self.last_press = Some((time_ms, (x, y), b));
                            out.push(Input::Press(e));
                        }
                    } else {
                        out.push(Input::Release(e));
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if inside(*pos) || self.held.iter().any(|h| *h) {
                        out.push(Input::Move(mouse(self, *pos)));
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let Some(p) = hover_pos.filter(|p| inside(*p)) else {
                        continue;
                    };
                    let (x, y) = local(p);
                    let notches = match unit {
                        egui::MouseWheelUnit::Line => delta.y as f64,
                        egui::MouseWheelUnit::Point => delta.y as f64 / 50.0,
                        egui::MouseWheelUnit::Page => delta.y as f64 * 3.0,
                    };
                    out.push(Input::Wheel {
                        x,
                        y,
                        delta: notches * 120.0,
                    });
                }
                egui::Event::Text(t) if keyboard_to_viewport => {
                    for c in t.chars() {
                        let k = if c == ' ' {
                            Key::Space
                        } else {
                            Key::Char(c.to_ascii_uppercase())
                        };
                        let mut ke = KeyEvent::new(k, &c.to_string(), m);
                        // the text already carries Shift
                        ke.modifiers.shift = false;
                        out.push(Input::KeyPress(ke.clone()));
                        out.push(Input::KeyRelease(ke));
                    }
                    typed_this_frame = true;
                }
                // egui-winit turns Ctrl+X / C / V into these and drops the
                // key, so the Cut, Copy and Paste shortcuts (and Ctrl+Alt+V)
                // would never fire: give the keys back.
                egui::Event::Cut | egui::Event::Copy | egui::Event::Paste(_)
                    if keyboard_to_viewport =>
                {
                    let c = match ev {
                        egui::Event::Cut => 'X',
                        egui::Event::Copy => 'C',
                        _ => 'V',
                    };
                    let mut em = m;
                    em.ctrl = true;
                    let ke = KeyEvent::new(Key::Char(c), "", em);
                    out.push(Input::KeyPress(ke.clone()));
                    out.push(Input::KeyRelease(ke));
                }
                egui::Event::Key {
                    key: k,
                    pressed,
                    repeat,
                    modifiers: em,
                    ..
                } if keyboard_to_viewport => {
                    let Some((kk, text)) = key(*k) else { continue };
                    let em = modifiers(*em);
                    // Plain typing arrives as Text; keys with Ctrl / Alt and
                    // the named keys come here.
                    let is_char = matches!(kk, Key::Char(_) | Key::Space);
                    if is_char && !em.ctrl && !em.alt {
                        continue;
                    }
                    let mut ke = KeyEvent::new(kk, if is_char { "" } else { &text }, em);
                    ke.auto_repeat = *repeat;
                    out.push(if *pressed {
                        Input::KeyPress(ke)
                    } else {
                        Input::KeyRelease(ke)
                    });
                }
                _ => {}
            }
        }
        let _ = typed_this_frame;
        out
    }
}
