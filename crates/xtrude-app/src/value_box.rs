// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The value box: typed values while a tool is active.
//!
//! Fields may carry a unit — `m`/`cm`/`mm`, `in` or `"`, `ft` or `'`,
//! feet-and-inches `1'6"`, fractions `3/4"`, mixed numbers `1 1/2"`. A bare
//! number is in the document's unit. `;` and whitespace separate fields;
//! a comma is a decimal separator except where a tool takes only several
//! values (`200,100` in the Rectangle).

use std::sync::OnceLock;

use regex::Regex;
use xtrude_model::units;

use crate::input::{Key, KeyEvent, Modifiers};

/// The largest number the value box takes (a UTM northing is ~1e7 m).
pub const MAX_TYPED: f64 = 1e9;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

macro_rules! regex {
    ($p:expr) => {{
        static CELL: OnceLock<Regex> = OnceLock::new();
        re(&CELL, $p)
    }};
}

/// `2`, `2.5`, `.5`, `3/4`, `1-1/2` → a number; anything else (or a
/// number too big for a model) → `None`.
pub fn parse_number(tok: &str) -> Option<f64> {
    let m = regex!(r"^(?:(\d+)-)?(\d+\.?\d*|\.\d+)(?:/(\d+\.?\d*))?$").captures(tok)?;
    let mut v: f64 = m.get(2)?.as_str().parse().ok()?;
    if let Some(d) = m.get(3) {
        let d: f64 = d.as_str().parse().ok()?;
        if d == 0.0 {
            return None;
        }
        v /= d;
    } else if m.get(1).is_some() {
        return None; // "1-2" without a fraction is not ours
    }
    if let Some(w) = m.get(1) {
        v += w.as_str().parse::<f64>().ok()?;
    }
    if v.is_finite() && v.abs() <= MAX_TYPED {
        Some(v)
    } else {
        None
    }
}

/// Ctrl, Alt or Meta held — a shortcut, not typing. Ctrl+Alt together is
/// AltGr, which types characters.
pub fn is_chord(m: Modifiers) -> bool {
    (m.ctrl || m.alt || m.meta) && !(m.ctrl && m.alt)
}

/// A whole number followed by a fraction field is ONE mixed number:
/// `1 1/2"` → `1-1/2"`, `1'6 1/2"` → `1'6-1/2"`, `1' 1/2"` → `1'1/2"`.
pub fn merge_mixed_numbers(fields: &[&str]) -> Vec<String> {
    let whole = regex!(r"^-?\d+(?:'\d*)?$");
    let frac = regex!(r#"^\d+/\d+(?:mm|cm|m|in|ft|"|')?$"#);
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let f = fields[i];
        if let Some(next) = fields.get(i + 1) {
            if whole.is_match(f) && frac.is_match(&next.to_lowercase()) {
                let joint = if f.ends_with('\'') { "" } else { "-" };
                out.push(format!("{f}{joint}{next}"));
                i += 2;
                continue;
            }
        }
        out.push(f.to_string());
        i += 1;
    }
    out
}

fn unit_scale(unit: &str) -> Option<f64> {
    Some(match unit {
        "m" => 1.0,
        "cm" => 0.01,
        "mm" => 0.001,
        "in" | "\"" => 0.0254,
        "ft" | "'" => 0.3048,
        _ => return None,
    })
}

/// One typed length in metres, or `None` when the field is not a length.
pub fn parse_length_field(field: &str) -> Option<f64> {
    let mut f = field.trim().to_lowercase();
    let mut sign = 1.0;
    if let Some(rest) = f.strip_prefix('-') {
        sign = -1.0;
        f = rest.to_string();
    }
    if f.is_empty() {
        return None;
    }
    if let Some(m) = regex!(r#"^([^'"]+)'([^'"]*)"?$"#).captures(&f) {
        let inches = m.get(2).map_or("", |x| x.as_str());
        if !inches.is_empty() {
            let feet = parse_number(m.get(1)?.as_str())?;
            let inches = parse_number(inches)?;
            return Some(sign * (feet * 0.3048 + inches * 0.0254));
        }
    }
    let m = regex!(r#"^(.+?)(mm|cm|m|in|ft|"|')?$"#).captures(&f)?;
    let num = parse_number(m.get(1)?.as_str())?;
    match m.get(2).map(|u| u.as_str()) {
        None | Some("") => Some(sign * num * units::bare_number_scale()),
        Some(u) => Some(sign * num * unit_scale(u)?),
    }
}

/// How an array repeats a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    /// `3x`, `*3`: more copies, each one more step beyond the last.
    External,
    /// `/3`: copies dividing the gap to the first copy into equal parts.
    Internal,
}

/// What a typed entry means.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    /// One length (metres) or plain number.
    Value(f64),
    /// Two fields (a rectangle's sides).
    Pair(f64, f64),
    /// Three fields (a delta).
    Triple(f64, f64, f64),
    /// `3x` / `*3` (external, optionally `step` metres apart) or `/3`
    /// (internal) — an array after a copy.
    Array {
        count: u32,
        kind: ArrayKind,
        step: Option<f64>,
    },
    /// `2r`: a radius.
    Radius(f64),
    /// `12s`: a segment / side count.
    Segments(u32),
    /// `3:12`: a slope, as degrees.
    Ratio(f64),
}

/// Parse a buffer (see the module doc). `comma_lists`: the tool takes
/// only several values, so `200,100` is two fields.
pub fn parse_value_buffer(buffer: &str, comma_lists: bool) -> Option<Typed> {
    let mut buffer = buffer.to_string();
    if comma_lists && !buffer.contains(';') && !buffer.trim().chars().any(char::is_whitespace) {
        buffer = buffer.replace(',', " ");
    }
    let normalized = buffer.replace(',', ".").replace(';', " ");
    let stripped = normalized.trim();
    let lower = stripped.to_lowercase();
    if let Some(m) = regex!(r"^(?:(\d+)\s*[x*]|[x*]\s*(\d+))$").captures(&lower) {
        let n = m.get(1).or(m.get(2))?.as_str().parse().ok()?;
        return Some(Typed::Array {
            count: n,
            kind: ArrayKind::External,
            step: None,
        });
    }
    if let Some(m) = regex!(r"^(\d+)\s*[x*]\s*(.+)$").captures(&lower) {
        let step = match parse_value_buffer(m.get(2)?.as_str(), false) {
            Some(Typed::Value(s)) if s > 0.0 => s,
            _ => return None,
        };
        return Some(Typed::Array {
            count: m.get(1)?.as_str().parse().ok()?,
            kind: ArrayKind::External,
            step: Some(step),
        });
    }
    if let Some(m) = regex!(r"^(?:/\s*(\d+)|(\d+)\s*/)$").captures(stripped) {
        let n = m.get(1).or(m.get(2))?.as_str().parse().ok()?;
        return Some(Typed::Array {
            count: n,
            kind: ArrayKind::Internal,
            step: None,
        });
    }
    if lower.ends_with('r') && !stripped.contains(':') {
        let m = regex!(r"^(\d+\.?\d*|\.\d+)r$").captures(&lower)?;
        return Some(Typed::Radius(m.get(1)?.as_str().parse().ok()?));
    }
    if lower.ends_with('s') && !stripped.contains(':') {
        let m = regex!(r"^(\d+)s$").captures(&lower)?;
        return Some(Typed::Segments(m.get(1)?.as_str().parse().ok()?));
    }
    if normalized.contains(':') {
        let m =
            regex!(r"^\s*(-?(?:\d+\.?\d*|\.\d+)):(\d+\.?\d*|\.\d+)\s*$").captures(&normalized)?;
        let rise: f64 = m.get(1)?.as_str().parse().ok()?;
        let run: f64 = m.get(2)?.as_str().parse().ok()?;
        if run <= 0.0 || !(rise.is_finite() && run.is_finite()) {
            return None;
        }
        return Some(Typed::Ratio(rise.atan2(run).to_degrees()));
    }
    let fields: Vec<&str> = normalized.split_whitespace().collect();
    let mut nums = Vec::new();
    for f in merge_mixed_numbers(&fields) {
        nums.push(parse_length_field(&f)?);
    }
    match nums.as_slice() {
        [a] => Some(Typed::Value(*a)),
        [a, b] => Some(Typed::Pair(*a, *b)),
        [a, b, c] => Some(Typed::Triple(*a, *b, *c)),
        _ => None,
    }
}

/// `M3`, `M2.5`, `M3c`: a screw size, or `None`.
pub fn parse_screw(buffer: &str) -> Option<Entry> {
    let m = regex!(r"^\s*[mM]\s*(\d+(?:[.,]\d+)?)\s*([cC])?\s*$").captures(buffer)?;
    let nominal: f64 = m.get(1)?.as_str().replace(',', ".").parse().ok()?;
    Some(Entry::Screw {
        nominal,
        close: m.get(2).is_some(),
    })
}

/// What the active tool takes in the value box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValueSpec {
    /// Angles, factors, counts: the document's unit stays out of it.
    pub unitless: bool,
    /// The tool takes only several values, so `200,100` is two fields.
    pub comma_lists: bool,
    /// `3x` / `/3` arrays, for a tool that lays them.
    pub array: Option<ArraySpec>,
    /// `12s`: a segment / side count.
    pub segments: bool,
    /// `2r`: a radius.
    pub radius: bool,
    /// `3:12`: a slope, handed over as degrees.
    pub angle_ratio: bool,
    /// `2` is a factor but `2m` an absolute size (Scale).
    pub absolute_length: bool,
    /// `M3`, `M3c`: a metric screw size (Hole).
    pub screw_sizes: bool,
}

/// How a tool takes arrays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArraySpec {
    /// The array may carry a spacing (`5x10m`).
    pub takes_step: bool,
}

/// What pressing Enter asks the viewport to hand the tool.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// `tool.on_value(value)`.
    Value(Typed),
    /// `tool.on_value(("abs_len", value))`.
    AbsoluteLength(Typed),
    /// `tool.on_value(degrees)` — only for tools taking a slope.
    Angle(f64),
    Segments(u32),
    Array {
        count: u32,
        kind: ArrayKind,
        step: Option<f64>,
    },
    Radius(f64),
    /// `M3` (normal fit) or `M3c` (close fit): a screw size, the nominal
    /// diameter in mm.
    Screw {
        nominal: f64,
        close: bool,
    },
    /// The entry was not understood (the buffer is cleared).
    Nothing,
}

/// The value box's buffer.
#[derive(Clone, Debug, Default)]
pub struct ValueBox {
    pub buffer: String,
}

impl ValueBox {
    /// The part of the buffer after the last `;` or space.
    pub fn current_token_tail(&self) -> &str {
        let normalized = self.buffer.replace(';', " ");
        match normalized.rfind(' ') {
            Some(i) => &self.buffer[i + 1..],
            None => &self.buffer,
        }
    }

    /// Enter: the entry the buffer means for a tool with `spec` (the
    /// buffer is cleared), or `None` with an empty buffer.
    pub fn commit(&mut self, spec: ValueSpec) -> Option<Entry> {
        if self.buffer.is_empty() {
            return None;
        }
        if spec.screw_sizes {
            if let Some(entry) = parse_screw(&self.buffer) {
                self.buffer.clear();
                return Some(entry);
            }
        }
        let value = if spec.unitless {
            units::unitless_numbers(|| parse_value_buffer(&self.buffer, spec.comma_lists))
        } else {
            parse_value_buffer(&self.buffer, spec.comma_lists)
        };
        let raw = std::mem::take(&mut self.buffer);
        Some(match value {
            None => Entry::Nothing,
            Some(Typed::Ratio(deg)) => {
                if spec.angle_ratio {
                    Entry::Angle(deg)
                } else {
                    Entry::Nothing
                }
            }
            Some(Typed::Segments(n)) => Entry::Segments(n),
            Some(Typed::Array { count, kind, step }) => Entry::Array { count, kind, step },
            Some(Typed::Radius(r)) => Entry::Radius(r),
            Some(v) => {
                let has_unit =
                    regex!(r#"[\d"']\s*(mm|cm|m|in|ft)\b|\d\s*["']"#).is_match(&raw.to_lowercase());
                if spec.absolute_length && has_unit {
                    Entry::AbsoluteLength(v)
                } else {
                    Entry::Value(v)
                }
            }
        })
    }

    /// A key while a tool is active: whether the value box took it (Enter
    /// is [`Self::commit`]'s; this handles typing and Backspace).
    pub fn handle_key(&mut self, ev: &KeyEvent, spec: ValueSpec) -> bool {
        if ev.key == Key::Backspace {
            if self.buffer.is_empty() {
                return false;
            }
            self.buffer.pop();
            return true;
        }
        if is_chord(ev.modifiers) {
            return false;
        }
        let text = ev.text.as_str();
        if spec.screw_sizes && self.buffer.is_empty() && text.eq_ignore_ascii_case("m") {
            self.buffer.push('M');
            return true;
        }
        if spec.array.is_some() {
            if text == "*" {
                self.buffer.push_str(text);
                return true;
            }
            if text.eq_ignore_ascii_case("x") && !self.current_token_tail().is_empty() {
                self.buffer.push_str(text);
                return true;
            }
            if text == "/" && self.buffer.is_empty() {
                self.buffer.push_str(text);
                return true;
            }
        }
        if text.chars().count() > 1 {
            // Several digits at once (an input method, a paste).
            if text.chars().all(|c| c.is_numeric()) {
                self.buffer.push_str(text);
                return true;
            }
            return false;
        }
        let Some(ch) = text.chars().next() else {
            return false;
        };
        let unit_letter = "mcrinft".contains(ch.to_ascii_lowercase());
        if !(ch.is_numeric() || ".,; -:\"'/".contains(ch) || unit_letter) {
            return false;
        }
        if (ch == ';' || ch == ' ') && self.buffer.is_empty() {
            return false;
        }
        let tail_empty = self.current_token_tail().is_empty();
        if unit_letter && tail_empty {
            return false;
        }
        if "\"'/".contains(ch) && tail_empty {
            return true;
        }
        if ch == '-' && !tail_empty {
            return true;
        }
        if ch == ':' && (tail_empty || self.buffer.contains(':')) {
            return true;
        }
        if ch == '.' || ch == ',' {
            let tail = self.current_token_tail();
            if tail.contains('.') || tail.contains(',') {
                return true;
            }
        }
        self.buffer.push(ch);
        true
    }
}
