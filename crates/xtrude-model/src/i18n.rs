// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! UI strings. The interface is English only (Xtrude CAD removed the
//! catalogues); [`tr`] marks a user-visible string and interpolates named
//! `{key}` placeholders.

/// `text` with `{name}` placeholders replaced from `args`. Without
/// arguments the text is returned unchanged (literal braces are safe).
pub fn tr(text: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    if args.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let key = &after[..end];
            if let Some((_, v)) = args.iter().find(|(k, _)| *k == key) {
                out.push_str(&v.to_string());
            } else {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// `tr!("Imported {name}", name = path)` — the keyword form.
#[macro_export]
macro_rules! tr {
    ($text:expr) => { $crate::i18n::tr($text, &[]) };
    ($text:expr, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::tr($text, &[$((stringify!($k), &$v as &dyn std::fmt::Display)),+])
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn interpolates_named_args() {
        assert_eq!(tr!("Imported {name}", name = "a.obj"), "Imported a.obj");
        assert_eq!(tr!("{a} and {b}", a = 1, b = 2.5), "1 and 2.5");
        assert_eq!(tr!("literal {braces}"), "literal {braces}");
    }
}
