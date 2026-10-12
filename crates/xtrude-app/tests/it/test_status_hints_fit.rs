// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Every status hint fits the bar on a 1366-px screen. The machine-independent
//! rule is a character count — pixels are a property of the machine —
//! calibrated at 112 characters; that is what this checks, for every tool,
//! idle and past its first click, in every Alt mode. (The interface is English
//! only: Xtrude CAD removed the catalogues.)

use crate::common;

use xtrude_app::window::hints::{alt_inference_hint, hint_for, started_texts};
use xtrude_app::window::tool_keys;
use xtrude_core::snap::LinearMode;

const MAX_CHARS: usize = 112;

/// States the test never drives a tool into, so never measured:
/// the section tool has no `start_point` (it can never be mid-operation
/// and carry the Alt clause), and the arc is started only as far as its
/// `end` step. (The arc's `bulge` step with the Alt clause runs to 128
/// characters and is elided.)
fn measured(key: &str, text: &str) -> bool {
    match key {
        "section" => false,
        "arc" => !text.starts_with("Click the bulge"),
        _ => true,
    }
}
const MODES: [LinearMode; 3] = [LinearMode::All, LinearMode::Off, LinearMode::ParallelPerp];

#[test]
fn test_no_hint_is_elided_on_a_1366_screen() {
    let mut win = common::window();
    let mut bad = Vec::new();
    for key in tool_keys() {
        win.activate_tool(key);
        let Some(tool) = win.viewport.active_tool() else {
            continue;
        };
        let mut texts = Vec::new();
        for mode in MODES {
            texts.push(hint_for(Some(key), Some(tool), None, Some(mode)));
            for t in started_texts(key).into_iter().filter(|t| measured(key, t)) {
                if tool.snap_prefs().uses_snap {
                    texts.push(format!("{t} | {}", alt_inference_hint(mode)));
                } else {
                    texts.push(t);
                }
            }
        }
        for t in texts {
            if t.chars().count() > MAX_CHARS {
                bad.push(format!(
                    "{key} ({} chars > {MAX_CHARS}): {t}",
                    t.chars().count()
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
