// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every character in a string literal of the app's sources is one the
//! UI font can draw: egui draws a character none of its fonts has as an
//! empty box.
//!
//! The fonts are egui's default ones (the shell loads none of its own),
//! read from `egui::FontDefinitions::default()`. A literal is checked
//! against the proportional family, the one almost all the UI uses; the
//! few literals that are not shown in it are listed in [`ELSEWHERE`],
//! with the place they do go to.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use skrifa::{FontRef, MetadataProvider};

/// Where a literal is shown.
#[derive(Clone, Copy, Debug)]
enum Shown {
    /// In the proportional family: every literal [`ELSEWHERE`] does not
    /// list.
    Proportional,
    /// Only in the monospace family (its characters are checked there).
    Monospace,
    /// Never on screen: a clipboard or file payload.
    OffScreen,
}

/// The literals that are not shown in the proportional family: the
/// source file (relative to `src/`), the literal's text and where it
/// goes. Each must still be in its file.
const ELSEWHERE: &[(&str, &str, Shown)] = &[
    // `stats_to_text`, the Model Info "Copy to clipboard" text; the
    // dialog itself shows the layers with other symbols
    ("window/model_info.rs", "✓", Shown::OffScreen),
    ("window/model_info.rs", "✗", Shown::OffScreen),
    ("window/model_info.rs", " 🔒", Shown::OffScreen),
];

/// The characters a font family maps, from egui's default fonts.
struct Family {
    fonts: Vec<Arc<egui::FontData>>,
}

impl Family {
    fn of(family: &egui::FontFamily) -> Self {
        let defs = egui::FontDefinitions::default();
        let names = defs.families.get(family).expect("a default family");
        let fonts = names
            .iter()
            .map(|name| defs.font_data.get(name).expect("a default font").clone())
            .collect::<Vec<_>>();
        assert!(!fonts.is_empty(), "{family:?} has no font");
        Self { fonts }
    }

    fn draws(&self, ch: char) -> bool {
        // control characters (line breaks, tabs) are layout, not glyphs
        ch.is_control()
            || self.fonts.iter().any(|data| {
                FontRef::from_index(&data.font, data.index)
                    .expect("a readable font")
                    .charmap()
                    .map(ch)
                    .is_some()
            })
    }
}

/// egui's two default families.
struct Fonts {
    proportional: Family,
    monospace: Family,
}

impl Fonts {
    fn load() -> Self {
        Self {
            proportional: Family::of(&egui::FontFamily::Proportional),
            monospace: Family::of(&egui::FontFamily::Monospace),
        }
    }

    /// The family a literal shown so is drawn in, if it is on screen.
    fn of(&self, shown: Shown) -> Option<&Family> {
        match shown {
            Shown::Proportional => Some(&self.proportional),
            Shown::Monospace => Some(&self.monospace),
            Shown::OffScreen => None,
        }
    }
}

/// A string or character literal: its decoded text and the line it
/// starts on.
#[derive(Debug)]
struct Literal {
    line: usize,
    text: String,
}

/// The string and character literals of Rust source `src`, outside
/// comments, with their escapes decoded.
fn literals(src: &str) -> Vec<Literal> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == 'r'
            && matches!(next, Some('"' | '#'))
            && (i == 0 || !is_ident(chars[i - 1]) || matches!(chars[i - 1], 'b' | 'c'))
        {
            // a raw string: r"…", r#"…"#
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) != Some(&'"') {
                // `r#ident`
                i = j;
                continue;
            }
            j += 1;
            let start_line = line;
            let mut text = String::new();
            while let Some(&ch) = chars.get(j) {
                if ch == '"' && (1..=hashes).all(|k| chars.get(j + k) == Some(&'#')) {
                    j += 1 + hashes;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                }
                text.push(ch);
                j += 1;
            }
            out.push(Literal {
                line: start_line,
                text,
            });
            i = j;
        } else if c == '"' {
            let start_line = line;
            let (text, end) = quoted(&chars, i + 1, '"', &mut line);
            out.push(Literal {
                line: start_line,
                text,
            });
            i = end;
        } else if c == '\'' {
            // a character literal, or a lifetime / label
            if next == Some('\\') || chars.get(i + 2) == Some(&'\'') {
                let (text, end) = quoted(&chars, i + 1, '\'', &mut line);
                out.push(Literal { line, text });
                i = end;
            } else {
                i += 1;
            }
        } else if is_ident(c) {
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            // a prefix of a raw string (`br"`, `cr"`) is read next round
            if i > 0 && chars[i - 1] == 'r' && matches!(chars.get(i), Some('"' | '#')) {
                i -= 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The text of a quoted literal from `start` (after the opening quote)
/// to the closing `quote`, escapes decoded; and the index after it.
fn quoted(chars: &[char], start: usize, quote: char, line: &mut usize) -> (String, usize) {
    let mut text = String::new();
    let mut j = start;
    while let Some(&ch) = chars.get(j) {
        if ch == quote {
            return (text, j + 1);
        }
        if ch == '\n' {
            *line += 1;
        }
        if ch != '\\' {
            text.push(ch);
            j += 1;
            continue;
        }
        let Some(&esc) = chars.get(j + 1) else { break };
        j += 2;
        match esc {
            'n' => text.push('\n'),
            't' => text.push('\t'),
            'r' => text.push('\r'),
            '0' => text.push('\0'),
            'x' => {
                let hex: String = chars[j..(j + 2).min(chars.len())].iter().collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    text.push(c);
                }
                j += 2;
            }
            'u' => {
                // \u{…}
                let close = chars[j..].iter().position(|&c| c == '}').unwrap_or(0);
                let hex: String = chars[j + 1..j + close].iter().collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    text.push(c);
                }
                j += close + 1;
            }
            '\n' => {
                // a line continuation skips the next line's indent
                *line += 1;
                while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                    if chars[j] == '\n' {
                        *line += 1;
                    }
                    j += 1;
                }
            }
            other => text.push(other),
        }
    }
    (text, j)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("a readable source folder")
        .map(|e| e.expect("a readable entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The characters of `lit` that the font it is `shown` in cannot draw,
/// as `line: 'c' (U+XXXX) in "text"` lines.
fn undrawable(lit: &Literal, shown: Shown, fonts: &Fonts) -> Vec<String> {
    let Some(family) = fonts.of(shown) else {
        return Vec::new();
    };
    let missing: BTreeSet<char> = lit.text.chars().filter(|&c| !family.draws(c)).collect();
    missing
        .into_iter()
        .map(|c| format!("{}: '{c}' (U+{:04X}) in {:?}", lit.line, c as u32, lit.text))
        .collect()
}

#[test]
fn test_the_ui_font_draws_every_character_of_the_app_strings() {
    let fonts = Fonts::load();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 50, "found only {} source files", files.len());

    let mut problems = Vec::new();
    let mut listed_seen = vec![false; ELSEWHERE.len()];
    for path in &files {
        let rel = path
            .strip_prefix(&src)
            .expect("a file under src")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(path).expect("a readable source file");
        for lit in literals(&text) {
            let listed = ELSEWHERE
                .iter()
                .position(|(file, listed, _)| *file == rel && *listed == lit.text);
            let shown = match listed {
                Some(k) => {
                    listed_seen[k] = true;
                    ELSEWHERE[k].2
                }
                None => Shown::Proportional,
            };
            for line in undrawable(&lit, shown, &fonts) {
                problems.push(format!("{rel}:{line} ({shown:?})"));
            }
        }
    }
    for (k, seen) in listed_seen.iter().enumerate() {
        if !seen {
            let (file, listed, shown) = ELSEWHERE[k];
            problems.push(format!(
                "{file}: the listed {shown:?} literal {listed:?} is gone; drop it from the list"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "characters the UI font cannot draw (an empty box on screen):\n{}",
        problems.join("\n")
    );
}

/// The check itself: an arrow the fonts lack is found, with its line;
/// comments are skipped, and escapes are decoded.
#[test]
fn test_the_scan_finds_a_missing_character() {
    let fonts = Fonts::load();
    let src = "// a comment ▸ is not shown\n\
               fn f() -> String {\n\
               \x20   let a = 'x';\n\
               \x20   format!(\"Edit \\u{25B8} Unhide {a} {}\", r#\"Camera ▸ \"raw\"\"#)\n\
               }\n";
    let lits = literals(src);
    let texts: Vec<&str> = lits.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["x", "Edit ▸ Unhide {a} {}", "Camera ▸ \"raw\""]);
    let found: Vec<String> = lits
        .iter()
        .flat_map(|lit| undrawable(lit, Shown::Proportional, &fonts))
        .collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("4: '▸' (U+25B8)"), "{found:?}");
    let plain = &literals("fn f() { \"Edit > Unhide — ok\"; }")[0];
    assert!(undrawable(plain, Shown::Proportional, &fonts).is_empty());
    // the monospace family has the triangle, but no check mark
    assert!(undrawable(&lits[1], Shown::Monospace, &fonts).is_empty());
    let check = &literals("fn f() { \"✓\"; }")[0];
    assert_eq!(undrawable(check, Shown::Monospace, &fonts).len(), 1);
    assert!(undrawable(check, Shown::OffScreen, &fonts).is_empty());
}
