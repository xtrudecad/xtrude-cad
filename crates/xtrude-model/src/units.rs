// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Length formatting shared by the model's dimensions and readouts.
//!
//! Model lengths are metres. A document's units decide how a bare typed
//! number is read and how every readout is shown: metric (`m`/`cm`/`mm`),
//! decimal imperial (`in`, `ft`, `ft-in`) or fractional imperial
//! (`in-frac` → `1 1/2"`, `ft-in-frac` → `1'6 1/2"`). For the fractional
//! forms the precision picks the finest denominator: 0 → whole inches,
//! 1 → 1/4, 2 → 1/16, 3 → 1/32, 4 → 1/64.
//!
//! So that tools can format without a scene at hand, the bound units are
//! a thread-local the scene updates whenever its units change.

use crate::settings::{self, keys, JsonText};
use std::cell::{Cell, RefCell};

use serde_json::Value;

pub const IN_M: f64 = 0.0254;
pub const FT_M: f64 = 0.3048;

/// Every unit a dimension style may name, in the order the pickers show.
pub const UNIT_CHOICES: [&str; 8] = [
    "m",
    "cm",
    "mm",
    "in",
    "ft",
    "ft-in",
    "in-frac",
    "ft-in-frac",
];

/// A document's units: the length unit and the display precision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Units {
    pub length: String,
    pub precision: i64,
}

impl Units {
    pub fn new(length: &str, precision: i64) -> Self {
        Units {
            length: length.to_string(),
            precision,
        }
    }

    /// `{"length": ..., "precision": ...}` for the document file.
    pub fn to_json(&self) -> Value {
        serde_json::json!({"length": self.length, "precision": self.precision})
    }
}

/// A new document's units: millimetres, the unit printed parts are drawn in.
pub fn default_model_units() -> Units {
    Units::new("mm", 2)
}

/// The units of an IngeTrazo `.igz` written before documents always
/// carried them.
pub fn legacy_document_units() -> Units {
    Units::new("m", 2)
}

pub fn unit_label(code: &str) -> String {
    match code {
        "m" => "Metres (m)",
        "cm" => "Centimetres (cm)",
        "mm" => "Millimetres (mm)",
        "in" => "Decimal inches (in)",
        "ft" => "Decimal feet (ft)",
        "ft-in" => "Feet and inches (1'6\")",
        "in-frac" => "Fractional inches",
        "ft-in-frac" => "Fractional feet and inches",
        other => other,
    }
    .to_string()
}

/// Metres per typed unit when the number carries no unit of its own.
pub fn bare_scale_of(unit: &str) -> Option<f64> {
    Some(match unit {
        "m" => 1.0,
        "cm" => 0.01,
        "mm" => 0.001,
        "in" | "ft-in" | "in-frac" | "ft-in-frac" => IN_M,
        "ft" => FT_M,
        _ => return None,
    })
}

/// The validated units of a raw document payload (`{"units": {...}}`).
pub fn model_units_of_payload(payload: &Value) -> Units {
    if let Some(u) = payload.get("units") {
        if let Some(len) = u.get("length").and_then(Value::as_str) {
            if bare_scale_of(len).is_some() {
                let prec = u.get("precision").and_then(Value::as_i64).unwrap_or(2);
                return Units::new(len, prec);
            }
        }
    }
    default_model_units()
}

/// Validate a units value (unknown unit → default).
pub fn validated(u: &Units) -> Units {
    if bare_scale_of(&u.length).is_some() {
        u.clone()
    } else {
        default_model_units()
    }
}

thread_local! {
    static BOUND: RefCell<Units> = RefCell::new(default_model_units());
    static OVERRIDE: RefCell<Option<Units>> = const { RefCell::new(None) };
    static NEW_DOC_OVERRIDE: RefCell<Option<Units>> = const { RefCell::new(None) };
    static UNITLESS: Cell<bool> = const { Cell::new(false) };
}

/// Bind the units the formatters read (the scene calls this).
pub fn bind(units: &Units) {
    BOUND.with(|b| *b.borrow_mut() = validated(units));
}

/// Test hook: force the units every
/// formatter and parser reads, and the ones a new document starts with.
pub fn set_override(units: Option<Units>) {
    OVERRIDE.with(|o| *o.borrow_mut() = units.clone());
    NEW_DOC_OVERRIDE.with(|o| *o.borrow_mut() = units);
}

pub fn model_units() -> Units {
    if let Some(u) = OVERRIDE.with(|o| o.borrow().clone()) {
        return u;
    }
    BOUND.with(|b| b.borrow().clone())
}

pub fn model_unit() -> String {
    model_units().length
}

pub fn model_precision() -> i64 {
    model_units().precision
}

/// The units a new, empty document starts with: the ones the user last
/// chose for new documents in Preferences, or millimetres.
pub fn new_document_units() -> Units {
    if let Some(u) = NEW_DOC_OVERRIDE.with(|o| o.borrow().clone()) {
        return u;
    }
    match settings::stored(&keys::NEW_DOCUMENT_UNITS) {
        Some(JsonText(v)) => model_units_of_payload(&serde_json::json!({ "units": v })),
        None => default_model_units(),
    }
}

pub fn remember_new_document_units(units: &Units) {
    let u = validated(units);
    settings::write(&keys::NEW_DOCUMENT_UNITS, &JsonText(u.to_json()));
    settings::sync();
}

/// Metres per unit for a number typed without a unit — and 1 while
/// [`unitless_numbers`] holds.
pub fn bare_number_scale() -> f64 {
    if UNITLESS.with(Cell::get) {
        return 1.0;
    }
    bare_scale_of(&model_unit()).unwrap_or(1.0)
}

/// Read typed numbers as they are while `f` runs: an angle, a scale factor
/// or a side count has no unit.
pub fn unitless_numbers<R>(f: impl FnOnce() -> R) -> R {
    let before = UNITLESS.with(|u| u.replace(true));
    let r = f();
    UNITLESS.with(|u| u.set(before));
    r
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs()
    } else {
        gcd(b, a % b)
    }
}

fn frac_inches(inches: f64, decimals: i64) -> String {
    let denom = match decimals.clamp(0, 4) {
        0 => 1,
        1 => 4,
        2 => 16,
        3 => 32,
        _ => 64,
    };
    let num = xtrude_core::math::round_even_int(inches * denom as f64);
    // Fraction(num, denom) normalised.
    let whole = num / denom; // int() truncates toward zero
    let rest_num = num - whole * denom;
    if rest_num == 0 {
        return whole.to_string();
    }
    let g = gcd(rest_num, denom);
    let (rn, rd) = (rest_num / g, denom / g);
    if whole == 0 {
        format!("{rn}/{rd}")
    } else {
        format!("{whole} {}/{rd}", rn.abs())
    }
}

/// `x` with `n` fixed decimals.
pub fn fmt_fixed(x: f64, n: usize) -> String {
    format!("{x:.n$}")
}

/// A displayed value rounded to `n` decimals, half to even.
fn round_n(x: f64, n: i64) -> f64 {
    xtrude_core::math::round_even(x, n as i32)
}

pub fn format_length(metres: f64, units: &str, decimals: i64) -> String {
    let n = decimals.clamp(0, 6);
    let nu = n as usize;
    let u = if units.is_empty() { "m" } else { units };
    match u {
        "in" => return format!("{}\"", fmt_fixed(metres / IN_M, nu)),
        "ft" => return format!("{}'", fmt_fixed(metres / FT_M, nu)),
        "ft-in" | "ft-in-frac" | "in-frac" => {
            let sign = if metres < 0.0 { "-" } else { "" };
            let total_in = metres.abs() / IN_M;
            if u == "in-frac" {
                return format!("{sign}{}\"", frac_inches(total_in, n));
            }
            let mut feet = (total_in / 12.0).floor() as i64;
            let mut inches = total_in - feet as f64 * 12.0;
            if u == "ft-in" {
                if round_n(inches, n) >= 12.0 {
                    feet += 1;
                    inches = 0.0;
                }
                return format!("{sign}{feet}'{}\"", fmt_fixed(inches, nu));
            }
            let mut txt = frac_inches(inches, n);
            if txt == "12" {
                feet += 1;
                txt = "0".into();
            }
            return format!("{sign}{feet}'{txt}\"");
        }
        _ => {}
    }
    let factor = match u {
        "m" => 1.0,
        "cm" => 100.0,
        "mm" => 1000.0,
        _ => 1.0,
    };
    let label = if matches!(u, "m" | "cm" | "mm") {
        u
    } else {
        "m"
    };
    format!("{} {}", fmt_fixed(metres * factor, nu), label)
}

/// A length in the model's units and precision — the one formatter every
/// readout uses.
pub fn fmt_len(metres: f64) -> String {
    let u = model_units();
    format_length(metres, &u.length, u.precision)
}

fn typed_mark(unit: &str) -> &'static str {
    match unit {
        "m" => " m",
        "cm" => " cm",
        "mm" => " mm",
        "in" | "ft-in" | "in-frac" | "ft-in-frac" => "\"",
        "ft" => "'",
        _ => " m",
    }
}

/// What the user is typing in the value box, as the canvas shows it: a bare number
/// (or a `a,b` / `a;b` list) gets the model's unit.
pub fn typed_value_text(buffer: &str) -> String {
    let text = buffer.trim();
    if !text.is_empty() && is_bare_number_list(text) {
        return format!("{text}{}", typed_mark(&model_unit()));
    }
    text.to_string()
}

/// `[-+]?[\d.]+(\s*[,;]\s*[-+]?[\d.]+)*`
fn is_bare_number_list(text: &str) -> bool {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c == ',' || c == ';' {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    parts.iter().all(|p| {
        let p = p.trim();
        let p = p.strip_prefix(['-', '+']).unwrap_or(p);
        !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.')
    }) && !text.starts_with(char::is_whitespace)
}

/// The number alone, for `a × b` pairs; imperial forms keep their marks.
pub fn fmt_num(metres: f64) -> String {
    let u = model_unit();
    match u.as_str() {
        "m" | "cm" | "mm" => {
            let factor = match u.as_str() {
                "m" => 1.0,
                "cm" => 100.0,
                _ => 1000.0,
            };
            fmt_fixed(metres * factor, model_precision().clamp(0, 20) as usize)
        }
        _ => fmt_len(metres),
    }
}

/// `3.00 × 2.00 m` (metric) or `3'0" × 2'0"` (imperial).
pub fn fmt_pair(a: f64, b: f64) -> String {
    let u = model_unit();
    if matches!(u.as_str(), "m" | "cm" | "mm") {
        format!("{} × {}", fmt_num(a), fmt_len(b))
    } else {
        format!("{} × {}", fmt_len(a), fmt_len(b))
    }
}

fn fine_decimals(unit: &str) -> i64 {
    match unit {
        "m" => 3,
        "cm" => 1,
        "mm" => 0,
        "in" | "ft-in" | "in-frac" | "ft-in-frac" => 2,
        "ft" => 3,
        _ => 3,
    }
}

/// The document's precision, raised to millimetre resolution.
pub fn fine_precision() -> i64 {
    model_precision().max(fine_decimals(&model_unit()))
}

pub fn fmt_len_fine(metres: f64) -> String {
    format_length(metres, &model_unit(), fine_precision())
}

/// `0.58 × 0.45 × 0.04 m` — length × width × thickness.
pub fn fmt_triple(a: f64, b: f64, c: f64, fine: bool) -> String {
    let u = model_unit();
    let n = if fine {
        fine_precision()
    } else {
        model_precision()
    };
    if matches!(u.as_str(), "m" | "cm" | "mm") {
        let factor = match u.as_str() {
            "m" => 1.0,
            "cm" => 100.0,
            _ => 1000.0,
        };
        let nu = n.clamp(0, 20) as usize;
        return format!(
            "{} × {} × {}",
            fmt_fixed(a * factor, nu),
            fmt_fixed(b * factor, nu),
            format_length(c, &u, n)
        );
    }
    [a, b, c]
        .iter()
        .map(|&v| format_length(v, &u, n))
        .collect::<Vec<_>>()
        .join(" × ")
}

/// An area in the model's units squared.
pub fn fmt_area(square_metres: f64) -> String {
    let u = model_unit();
    let n = model_precision().clamp(0, 20) as usize;
    match u.as_str() {
        "m" | "cm" | "mm" => {
            let factor = match u.as_str() {
                "m" => 1.0,
                "cm" => 1e4,
                _ => 1e6,
            };
            format!("{} {u}²", fmt_fixed(square_metres * factor, n))
        }
        s if s.starts_with("ft") => format!("{} ft²", fmt_fixed(square_metres / (FT_M * FT_M), n)),
        _ => format!("{} in²", fmt_fixed(square_metres / (IN_M * IN_M), n)),
    }
}

/// A volume in the model's units cubed, except that mm and cm documents
/// read cm³, the unit a printed part's volume is usually given in.
pub fn fmt_volume(cubic_metres: f64) -> String {
    let u = model_unit();
    let n = model_precision().clamp(0, 20) as usize;
    match u.as_str() {
        "m" => format!("{} m³", fmt_fixed(cubic_metres, n)),
        "cm" | "mm" => format!("{} cm³", fmt_fixed(cubic_metres * 1e6, n)),
        s if s.starts_with("ft") => {
            format!("{} ft³", fmt_fixed(cubic_metres / (FT_M * FT_M * FT_M), n))
        }
        _ => format!("{} in³", fmt_fixed(cubic_metres / (IN_M * IN_M * IN_M), n)),
    }
}

/// RAII helper for tests: bind metre units for this thread while alive.
pub struct UnitsOverride;

impl UnitsOverride {
    pub fn metres() -> Self {
        set_override(Some(legacy_document_units()));
        UnitsOverride
    }

    pub fn with(units: Units) -> Self {
        set_override(Some(units));
        UnitsOverride
    }
}

impl Drop for UnitsOverride {
    fn drop(&mut self) {
        set_override(None);
    }
}
