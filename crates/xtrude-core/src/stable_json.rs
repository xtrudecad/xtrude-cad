// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! JSON text whose bytes are fixed: `", "` and `": "` separators, every
//! non-ASCII code point escaped, floats in their shortest round-trip form
//! (`1.0`, `1e-05`, `1e+16`), and correctly rounded decimals.
//!
//! Needed where a document stores a digest of a JSON text (the 3D-text
//! fingerprint): a fingerprint saved in a file must verify the same way
//! here, so these bytes never change.

/// A JSON value (object keys keep insertion order unless dumped with
/// `sort_keys`).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Dict(Vec<(String, Json)>),
}

/// `x` rounded to `ndigits` decimals, correctly (half to even on the exact
/// binary value), sign of zero kept.
pub fn round_exact(x: f64, ndigits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.ndigits$}").parse().unwrap_or(x)
}

/// A float as text: the shortest round-tripping digits, always with a
/// decimal point or an exponent, exponent form when the decimal point
/// sits below 1e-4 or at 1e16 and beyond (`1e-05`, `1e+16`).
pub fn float_text(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Rust's `{:e}` is the shortest round-trip: "d.ddde±x"
    let s = format!("{:e}", x.abs());
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1; // digits d1d2... with the point after decpt digits
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if !(-3..=16).contains(&decpt) {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.clone()
        };
        let e = decpt - 1;
        format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    } else if decpt <= 0 {
        format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
    } else if (decpt as usize) >= digits.len() {
        format!("{}{}.0", digits, "0".repeat(decpt as usize - digits.len()))
    } else {
        let (a, b) = digits.split_at(decpt as usize);
        format!("{a}.{b}")
    };
    format!("{sign}{body}")
}

/// A string as JSON text: quotes, escapes, every non-ASCII code
/// point as `\uXXXX` (surrogate pairs above the BMP).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `value` as JSON text, object keys sorted when `sort_keys`.
pub fn dump(value: &Json, sort_keys: bool) -> String {
    let mut out = String::new();
    write(value, sort_keys, &mut out);
    out
}

fn write(v: &Json, sort_keys: bool, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) => out.push_str(&float_text(*f)),
        Json::Str(s) => out.push_str(&quote(s)),
        Json::List(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(it, sort_keys, out);
            }
            out.push(']');
        }
        Json::Dict(items) => {
            let mut refs: Vec<&(String, Json)> = items.iter().collect();
            if sort_keys {
                refs.sort_by(|a, b| a.0.cmp(&b.0));
            }
            out.push('{');
            for (i, (k, it)) in refs.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&quote(k));
                out.push_str(": ");
                write(it, sort_keys, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_in_shortest_round_trip_form() {
        let cases = [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (-2.5, "-2.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.2345e17, "1.2345e+17"),
            (123.456, "123.456"),
            (-0.0, "-0.0"),
            (0.30000000000000004, "0.30000000000000004"),
        ];
        for (x, want) in cases {
            assert_eq!(float_text(x), want, "{x}");
        }
    }

    #[test]
    fn round_exact_is_correctly_rounded() {
        assert_eq!(round_exact(2.675, 2), 2.67); // binary value below the tie
        assert_eq!(round_exact(0.125, 2), 0.12); // exact tie: to even
        assert_eq!(round_exact(0.375, 2), 0.38);
        assert!(round_exact(-0.00001, 4).is_sign_negative());
    }

    #[test]
    fn dump_is_byte_exact() {
        let v = Json::List(vec![
            Json::Str("é\"x".into()),
            Json::Int(3),
            Json::List(vec![Json::Float(0.5), Json::Null]),
            Json::Dict(vec![
                ("b".into(), Json::Bool(false)),
                ("a".into(), Json::Str("😀".into())),
            ]),
        ]);
        assert_eq!(
            dump(&v, true),
            "[\"\\u00e9\\\"x\", 3, [0.5, null], {\"a\": \"\\ud83d\\ude00\", \"b\": false}]"
        );
    }
}
