// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The main window's actions — menu entries, toolbar buttons, shortcuts —
//! as data, and key sequences («Ctrl+Shift+Z»).
//!
//! Two actions must never hold the same keys: a key that two actions
//! share fires neither, rather than guessing one ([`ActionList::for_key`]).

use crate::input::{Key, KeyEvent, Modifiers};

/// One key chord: modifiers plus a key («Ctrl+Shift+Z»).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySeq {
    pub mods: Modifiers,
    pub key: Key,
}

impl KeySeq {
    pub const fn new(mods: Modifiers, key: Key) -> KeySeq {
        KeySeq { mods, key }
    }

    /// Parse the portable text form («Shift+R», «Ctrl+Alt+V», «F2»,
    /// «Esc», «Space»). `None` for an empty or unreadable text.
    pub fn parse(text: &str) -> Option<KeySeq> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut mods = Modifiers::NONE;
        let parts: Vec<&str> = text.split('+').collect();
        // a literal "+" key ("Ctrl++") leaves an empty last part
        let (last, rest) = if text.ends_with("++") {
            ("+", &parts[..parts.len() - 2])
        } else {
            (parts[parts.len() - 1], &parts[..parts.len() - 1])
        };
        for m in rest {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "shift" => mods.shift = true,
                "alt" => mods.alt = true,
                "meta" | "cmd" | "super" => mods.meta = true,
                _ => return None,
            }
        }
        let key = parse_key(last.trim())?;
        Some(KeySeq { mods, key })
    }

    /// The text form shown in menus («Ctrl+Shift+Z»).
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.mods.ctrl {
            s.push_str("Ctrl+");
        }
        if self.mods.alt {
            s.push_str("Alt+");
        }
        if self.mods.shift {
            s.push_str("Shift+");
        }
        if self.mods.meta {
            s.push_str("Meta+");
        }
        s.push_str(&key_name(self.key));
        s
    }

    /// Whether a key press fires this sequence.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        normalise(ev.key) == normalise(self.key)
            && ev.modifiers.ctrl == self.mods.ctrl
            && ev.modifiers.alt == self.mods.alt
            && ev.modifiers.shift == self.mods.shift
            && ev.modifiers.meta == self.mods.meta
    }
}

fn normalise(k: Key) -> Key {
    match k {
        Key::Enter => Key::Return,
        Key::Char(c) => Key::Char(c.to_ascii_uppercase()),
        other => other,
    }
}

fn parse_key(s: &str) -> Option<Key> {
    let low = s.to_ascii_lowercase();
    Some(match low.as_str() {
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "return" => Key::Return,
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "del" | "delete" => Key::Delete,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "pgup" | "pageup" => Key::PageUp,
        "pgdown" | "pagedown" => Key::PageDown,
        _ => {
            if let Some(n) = low.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                if (1..=35).contains(&n) {
                    return Some(Key::F(n));
                }
            }
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Char(c.to_ascii_uppercase())
        }
    })
}

fn key_name(k: Key) -> String {
    match k {
        Key::Escape => "Esc".into(),
        Key::Space => "Space".into(),
        Key::Return => "Return".into(),
        Key::Enter => "Enter".into(),
        Key::Tab => "Tab".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Del".into(),
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Up => "Up".into(),
        Key::Down => "Down".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PgUp".into(),
        Key::PageDown => "PgDown".into(),
        Key::F(n) => format!("F{n}"),
        Key::Char(c) => c.to_string(),
        Key::Shift => "Shift".into(),
        Key::Control => "Ctrl".into(),
        Key::Alt => "Alt".into(),
        Key::Meta => "Meta".into(),
    }
}

/// The standard desktop keys (Linux values).
pub mod std_keys {
    pub const NEW: &str = "Ctrl+N";
    pub const OPEN: &str = "Ctrl+O";
    pub const SAVE: &str = "Ctrl+S";
    pub const SAVE_AS: &str = "Ctrl+Shift+S";
    pub const QUIT: &str = "Ctrl+Q";
    pub const UNDO: &str = "Ctrl+Z";
    pub const CUT: &str = "Ctrl+X";
    pub const COPY: &str = "Ctrl+C";
    pub const PASTE: &str = "Ctrl+V";
    pub const SELECT_ALL: &str = "Ctrl+A";
}

/// One pressable action.
#[derive(Clone, Debug)]
pub struct Action {
    /// Stable name (the object name, or the command key).
    pub id: String,
    pub text: String,
    /// What it does — the status bar line and the second tooltip line.
    pub status_tip: String,
    pub shortcuts: Vec<KeySeq>,
    /// A menu draws the check mark only for a checkable action.
    pub checkable: bool,
    /// Pressed on a toolbar. `checked` without `checkable`: pressed on a
    /// toolbar, no check mark in a menu (Toggle X-ray).
    pub checked: bool,
    pub enabled: bool,
    pub icon: Option<String>,
}

impl Action {
    pub fn new(id: &str, text: &str) -> Action {
        Action {
            id: id.to_string(),
            text: text.to_string(),
            status_tip: String::new(),
            shortcuts: Vec::new(),
            checkable: false,
            checked: false,
            enabled: true,
            icon: None,
        }
    }

    pub fn tip(mut self, tip: &str) -> Action {
        self.status_tip = tip.to_string();
        self
    }

    pub fn keys(mut self, keys: &[&str]) -> Action {
        self.shortcuts = keys.iter().filter_map(|k| KeySeq::parse(k)).collect();
        self
    }

    pub fn checkable(mut self, checked: bool) -> Action {
        self.checkable = true;
        self.checked = checked;
        self
    }

    pub fn icon(mut self, icon: &str) -> Action {
        self.icon = Some(icon.to_string());
        self
    }

    /// The first shortcut's text, "" without one.
    pub fn shortcut_text(&self) -> String {
        self.shortcuts.first().map(KeySeq::text).unwrap_or_default()
    }

    /// A toolbar tooltip naming the action's CURRENT keys.
    pub fn tooltip(&self) -> String {
        let keys = self.shortcut_text();
        let mut tip = if keys.is_empty() {
            self.text.clone()
        } else {
            format!("{}  ({keys})", plain(&self.text))
        };
        if !self.status_tip.is_empty() {
            tip.push('\n');
            tip.push_str(&self.status_tip);
        }
        tip
    }
}

/// A menu's text without accelerators or a trailing ellipsis.
pub fn plain(text: &str) -> String {
    text.replace("&&", "\0")
        .replace('&', "")
        .replace('\0', "&")
        .trim_end_matches('…')
        .trim_end_matches('.')
        .trim()
        .to_string()
}

/// A menu entry.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Action(String),
    Menu(String, Vec<MenuItem>),
    Separator,
    /// Filled when shown (Open Recent, Panels).
    Dynamic(&'static str),
}

/// The window's actions, in creation order.
#[derive(Default, Debug)]
pub struct ActionList {
    list: Vec<Action>,
}

impl ActionList {
    pub fn add(&mut self, a: Action) {
        if let Some(old) = self.list.iter_mut().find(|x| x.id == a.id) {
            *old = a;
        } else {
            self.list.push(a);
        }
    }

    /// Keep only the actions `keep` accepts.
    pub fn retain(&mut self, keep: impl FnMut(&Action) -> bool) {
        self.list.retain(keep);
    }

    pub fn get(&self, id: &str) -> Option<&Action> {
        self.list.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Action> {
        self.list.iter_mut().find(|a| a.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Action> {
        self.list.iter()
    }

    /// The enabled actions a key press fires. More than one is an
    /// ambiguity: the caller fires none.
    pub fn for_key(&self, ev: &KeyEvent) -> Vec<&Action> {
        self.list
            .iter()
            .filter(|a| a.enabled && a.shortcuts.iter().any(|s| s.matches(ev)))
            .collect()
    }

    /// Every action holding `seq`.
    pub fn owners(&self, seq: KeySeq) -> Vec<&Action> {
        self.list
            .iter()
            .filter(|a| a.shortcuts.contains(&seq))
            .collect()
    }
}
