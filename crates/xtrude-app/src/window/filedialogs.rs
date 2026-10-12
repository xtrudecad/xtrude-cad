// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File dialogs that open where you last were.
//!
//! A start without a folder part goes into the last folder the user chose
//! in ANY file dialog (setting `files/last_dir`), falling back to the open
//! document's folder, then to Documents, then home; a start that names a
//! folder is respected. Whatever the user picks is remembered.

use std::path::{Path, PathBuf};

use xtrude_model::settings::{self, keys};

/// The user's Documents folder (`$XDG_DOCUMENTS_DIR`, else `~/Documents`).
fn documents() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_DOCUMENTS_DIR") {
        let d = PathBuf::from(d);
        if d.is_dir() {
            return Some(d);
        }
    }
    let home = home()?;
    let d = home.join("Documents");
    d.is_dir().then_some(d)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The folder to start a dialog in: the last one chosen, if it still
/// exists; else `fallback`; else Documents; else home.
pub fn last_dir(fallback: Option<&Path>) -> PathBuf {
    if let Some(saved) = settings::stored(&keys::LAST_DIR).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(saved);
        if p.is_dir() {
            return p;
        }
    }
    if let Some(f) = fallback.filter(|f| f.is_dir()) {
        return f.to_path_buf();
    }
    documents()
        .or_else(home)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Keep the folder of a chosen file (or the chosen folder itself).
pub fn remember(path: Option<&Path>) {
    let Some(p) = path else { return };
    if p.as_os_str().is_empty() {
        return;
    }
    let folder = if p.is_dir() {
        p
    } else {
        match p.parent() {
            Some(f) => f,
            None => return,
        }
    };
    if folder.is_dir() {
        settings::write(&keys::LAST_DIR, &folder.to_string_lossy().into_owned());
    }
}

/// Resolve a dialog's start: nothing or a bare name goes into the
/// remembered folder; a real folder (or a file inside one) stays.
pub fn start_in(directory: &str, fallback: Option<&Path>) -> PathBuf {
    if directory.is_empty() {
        return last_dir(fallback);
    }
    let d = Path::new(directory);
    if let Some(head) = d.parent().filter(|h| !h.as_os_str().is_empty()) {
        if head.is_dir() {
            return d.to_path_buf(); // the caller chose
        }
    }
    let base = d.file_name().map(PathBuf::from).unwrap_or_default();
    last_dir(fallback).join(base)
}
