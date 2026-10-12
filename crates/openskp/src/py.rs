// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The few Python built-in behaviours the parser's output depends on:
//! `str(float)`, `int(str)`, `float(str)`, `bytes.decode` with an error
//! handler, and `min`/`max` with NaN.

/// `repr(x)` / `str(x)` of a Python `float`: the shortest round-trip
/// digits, in positional notation for exponents from -4 to 15 and in
/// scientific notation (`1e+16`, `1.5e-05`) outside that range.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // `{:e}` gives the shortest round-trip digits: "-1.2345e-7".
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exp) {
        let n = digits.len() as i32;
        let body = if exp < 0 {
            format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
        } else if exp + 1 >= n {
            format!("{}{}.0", digits, "0".repeat((exp + 1 - n) as usize))
        } else {
            let (int, frac) = digits.split_at((exp + 1) as usize);
            format!("{int}.{frac}")
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let mant = if rest.is_empty() {
            first.to_string()
        } else {
            format!("{first}.{rest}")
        };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{mant}e{esign}{:02}", exp.abs())
    }
}

/// The digits of a Python numeric literal with its optional `_`
/// separators removed, or `None` when an underscore is misplaced.
fn strip_underscores(s: &str) -> Option<String> {
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    Some(s.replace('_', ""))
}

/// `int(s)` for a base-10 string: surrounding whitespace, a sign and `_`
/// separators allowed. `None` where Python raises `ValueError` (and, as a
/// limit of this crate, for values outside `i64`).
pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = strip_underscores(digits)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// `int(s) & 0xFFFFFFFF`: the low 32 bits of a decimal integer of any size
/// (two's complement for negative values), or `None` on `ValueError`.
pub fn parse_int_low32(s: &str) -> Option<u32> {
    let t = s.trim();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = strip_underscores(digits)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let m = digits
        .bytes()
        .fold(0u64, |acc, b| (acc * 10 + u64::from(b - b'0')) % (1 << 32));
    Some(if neg {
        ((1u64 << 32) - m) as u32
    } else {
        m as u32
    })
}

/// `float(s)`: decimal or scientific notation, `inf`/`nan` spellings,
/// surrounding whitespace and `_` separators. `None` on `ValueError`.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let body = lower.trim_start_matches(['+', '-']);
    if body.starts_with("inf") || body.starts_with("nan") {
        return ["inf", "infinity", "nan"]
            .contains(&body)
            .then(|| t.parse::<f64>().ok())
            .flatten();
    }
    let cleaned = if t.contains('_') {
        // Underscores may only sit between two digits.
        let b = t.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            if c == b'_' {
                let ok = i > 0
                    && i + 1 < b.len()
                    && b[i - 1].is_ascii_digit()
                    && b[i + 1].is_ascii_digit();
                if !ok {
                    return None;
                }
            }
        }
        t.replace('_', "")
    } else {
        t.to_string()
    };
    let valid = cleaned
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !valid {
        return None;
    }
    cleaned.parse().ok()
}

/// `bytes.decode('utf-8', errors='replace')`.
pub fn utf8_replace(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `bytes.decode('utf-16-le', errors=…)`: `replace` turns each unpaired
/// surrogate (and a trailing odd byte) into U+FFFD, `ignore` drops them.
pub fn utf16le(b: &[u8], replace: bool) -> String {
    let units = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]));
    let mut out = String::with_capacity(b.len() / 2);
    for c in char::decode_utf16(units) {
        match c {
            Ok(c) => out.push(c),
            Err(_) if replace => out.push(char::REPLACEMENT_CHARACTER),
            Err(_) => {}
        }
    }
    if replace && b.len() % 2 == 1 {
        out.push(char::REPLACEMENT_CHARACTER);
    }
    out
}

/// Python's `max(a, b)`: `a` unless `b > a` (so a NaN `a` survives).
pub fn max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// Python's `min(a, b)`: `a` unless `b < a`.
pub fn min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `bytes.hex().upper()`.
pub fn hex_upper(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (3.5, "3.5"),
            (10.0, "10.0"),
            (0.1, "0.1"),
            (-2.25, "-2.25"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.2345e-7, "1.2345e-07"),
            (123456.789, "123456.789"),
            (1e300, "1e+300"),
            (-0.0, "-0.0"),
        ];
        for (x, want) in cases {
            assert_eq!(float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn int_and_float_parse_like_python() {
        assert_eq!(parse_int(" 12 "), Some(12));
        assert_eq!(parse_int("-7"), Some(-7));
        assert_eq!(parse_int("1_000"), Some(1000));
        assert_eq!(parse_int("1.0"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("_1"), None);
        assert_eq!(parse_int_low32("-3552052"), Some(-3552052i64 as u32));
        assert_eq!(parse_int_low32("4294967297"), Some(1));
        assert_eq!(parse_float("0.27"), Some(0.27));
        assert_eq!(parse_float(" 2.75 "), Some(2.75));
        assert_eq!(parse_float("1e3"), Some(1000.0));
        assert!(parse_float("nan").is_some_and(f64::is_nan));
        assert_eq!(parse_float("abc"), None);
        assert_eq!(parse_float("1_0.5"), Some(10.5));
    }
}
