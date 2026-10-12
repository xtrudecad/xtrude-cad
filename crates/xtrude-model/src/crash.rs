// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The crash black box: a panic appends what happened to
//! `xtrude-crash.log` in the log folder ([`crate::paths::user_log_dir`]),
//! then the default hook runs as before. A panic a command catches and
//! rolls back is logged too.

use std::io::Write;

/// The crash log's file name in the log folder.
pub const CRASH_LOG: &str = "xtrude-crash.log";

/// One crash log entry: when, the version, the message, where, and the
/// backtrace.
pub fn entry(when: &str, message: &str, location: Option<&str>, backtrace: &str) -> String {
    let mut text = format!(
        "[{when}] Xtrude CAD {} panicked: {message}\n",
        crate::VERSION
    );
    if let Some(at) = location {
        text += &format!("at {at}\n");
    }
    let backtrace = backtrace.trim();
    if !backtrace.is_empty() {
        text += backtrace;
        text.push('\n');
    }
    text.push('\n');
    text
}

/// Install the panic hook that writes the crash log, keeping the current
/// hook (the default message on stderr) after it.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let text = entry(
            &crate::history::now_iso(),
            &message,
            location.as_deref(),
            &backtrace,
        );
        if let Some(mut log) = crate::paths::open_crash_log(CRASH_LOG) {
            let _ = log.write_all(text.as_bytes());
        }
        previous(info);
    }));
}
