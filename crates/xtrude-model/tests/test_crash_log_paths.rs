// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Logs land in the user's state folder, never the install or working
//! directory; the app's panic hook writes to the same crash log.
//!
//! A test binary of its own, outside `tests/it`: it changes the working
//! directory and `XDG_STATE_HOME`, which would leak into other tests.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use xtrude_model::history::{CmdError, CmdResult, Command, History};
use xtrude_model::paths;
use xtrude_model::scene::Scene;

/// The tests change process-wide environment and cwd: one at a time.
static ENV: Mutex<()> = Mutex::new(());

fn state_home() -> (tempdir::Dir, PathBuf) {
    let tmp = tempdir::Dir::new();
    let d = tmp.0.join("state");
    std::env::set_var("XDG_STATE_HOME", &d);
    (tmp, d)
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "xtrude-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&p).expect("temp dir");
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn test_user_log_dir_is_writable_and_outside_the_app() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (_tmp, state) = state_home();
    let d = paths::user_log_dir().unwrap();
    assert!(d.is_absolute() && d.is_dir());
    assert!(!d.starts_with(paths::app_root().canonicalize().unwrap()));
    assert_eq!(d, state.join("xtrude"));
    std::fs::write(d.join("probe"), "x").unwrap();
}

#[test]
fn test_crash_log_opens_with_a_read_only_cwd_and_no_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (tmp, _state) = state_home();
    let ro = tmp.0.join("program-files");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
    let old_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&ro).unwrap();
    let writable = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("xtrude-crash.log")
        .is_ok();
    let result = std::panic::catch_unwind(|| {
        if writable {
            return; // running as a user that writes anywhere
        }
        let mut fh = paths::open_crash_log("xtrude-crash.log").expect("a crash log");
        fh.write_all(b"ok\n").unwrap();
        drop(fh);
        let text = std::fs::read_to_string(paths::user_log_dir().unwrap().join("xtrude-crash.log"))
            .unwrap();
        assert!(text.ends_with("ok\n"));
    });
    std::env::set_current_dir(old_cwd).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
    result.unwrap();
}

struct Boom;

impl Command for Boom {
    fn name(&self) -> &str {
        "Boom"
    }
    fn apply(&mut self, _scene: &mut Scene) -> CmdResult {
        Err(CmdError::msg("kaboom"))
    }
}

#[test]
fn test_failed_commands_log_into_the_user_folder() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (_tmp, _state) = state_home();
    let mut s = Scene::new();
    let mut h = History::new();
    h.execute(&mut s, &mut Boom);
    assert!(h
        .last_error
        .as_deref()
        .is_some_and(|e| e.contains("kaboom")));
    let log = History::default_error_log().unwrap();
    assert_eq!(log.parent().unwrap(), paths::user_log_dir().unwrap());
    assert!(std::fs::read_to_string(log).unwrap().contains("kaboom"));
}

#[test]
fn test_no_default_writes_land_in_the_working_directory() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (tmp, _state) = state_home();
    let old_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&tmp.0).unwrap();
    let here = !std::path::Path::new("xtrude-errors.log").exists();
    let absolute = History::default_error_log().unwrap().is_absolute();
    std::env::set_current_dir(old_cwd).unwrap();
    assert!(here && absolute);
}

#[test]
fn test_a_panic_lands_in_the_crash_log() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (_tmp, _state) = state_home();
    xtrude_model::crash::install_panic_hook();
    let caught = std::panic::catch_unwind(|| panic!("boom-for-the-crash-log"));
    let _ = std::panic::take_hook(); // back to the default hook
    assert!(caught.is_err());
    let log = paths::user_log_dir()
        .unwrap()
        .join(xtrude_model::crash::CRASH_LOG);
    let text = std::fs::read_to_string(log).expect("the crash log");
    assert!(text.contains("boom-for-the-crash-log"), "{text}");
    assert!(text.contains(xtrude_model::VERSION), "{text}");
    assert!(text.contains("test_crash_log_paths.rs:"), "{text}");
}

#[test]
fn test_a_crash_entry_says_when_what_and_where() {
    let e = xtrude_model::crash::entry("2026-10-09T12:00:00Z", "oops", Some("a.rs:1:2"), "");
    assert_eq!(
        e,
        format!(
            "[2026-10-09T12:00:00Z] Xtrude CAD {} panicked: oops\nat a.rs:1:2\n\n",
            xtrude_model::VERSION
        )
    );
}
