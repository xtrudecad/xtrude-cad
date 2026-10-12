// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Where the application's data and log files live.

use std::path::PathBuf;

/// A writable folder for the black-box logs (crash / failed commands):
/// `$XDG_STATE_HOME/xtrude` or `~/.local/state/xtrude`; the temp dir as
/// the last resort.
pub fn user_log_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local").join("state")));
    let mut candidates = Vec::new();
    if let Some(s) = state {
        candidates.push(s.join("xtrude"));
    }
    candidates.push(std::env::temp_dir().join("xtrude"));
    for d in candidates {
        if std::fs::create_dir_all(&d).is_ok() {
            let probe = d.join(".write-test");
            if std::fs::write(&probe, b"").is_ok() {
                let _ = std::fs::remove_file(&probe);
                return Some(d);
            }
        }
    }
    Some(std::env::temp_dir())
}

/// The directory holding `resources/` (shaders, colours, icons):
/// `$XTRUDE_RESOURCES/..` when set, else next to the executable (or its
/// `../share/xtrude`), else the source tree this binary was built from.
pub fn app_root() -> PathBuf {
    if let Some(r) = std::env::var_os("XTRUDE_RESOURCES") {
        let r = PathBuf::from(r);
        if let Some(parent) = r.parent() {
            return parent.to_path_buf();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                dir.to_path_buf(),
                dir.join("..").join("share").join("xtrude"),
            ] {
                if cand.join("resources").is_dir() {
                    return cand;
                }
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// `resources/<rel>` under [`app_root`].
pub fn resource(rel: &str) -> PathBuf {
    app_root().join("resources").join(rel)
}

/// An append handle for the crash black box ([`crate::crash`] writes there),
/// or `None` when no location is writable at all.
pub fn open_crash_log(name: &str) -> Option<std::fs::File> {
    let dir = user_log_dir()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
        .ok()
}
