// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Helpers shared by the tests.

use std::path::PathBuf;
use std::time::Duration;

use mlua::Value;
use xtrude_ext::{Limits, Runtime, ScriptCommand};
use xtrude_model::{History, Scene};

/// A runtime with small limits, so that the time-limit tests end quickly.
pub fn tight_runtime() -> Runtime {
    Runtime::with_limits(Limits {
        memory: 16 << 20,
        time: Duration::from_millis(200),
    })
    .expect("Lua runtime")
}

/// A runtime with the same small memory limit as `tight_runtime()` and a
/// time limit long enough that it cannot fire first: filling 16 MiB takes
/// longer than 200 ms on a busy machine, and the memory tests must see the
/// memory limit whatever the load.
pub fn memory_runtime() -> Runtime {
    Runtime::with_limits(Limits {
        memory: 16 << 20,
        time: Duration::from_secs(60),
    })
    .expect("Lua runtime")
}

/// A temporary directory, removed on drop.
pub struct TmpDir(pub PathBuf);

impl TmpDir {
    pub fn new() -> TmpDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "xtrude-ext-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TmpDir(p)
    }

    /// Writes `text` to `rel` under the directory, creating its folders.
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("temp subdir");
        }
        std::fs::write(&path, text).expect("temp file");
        path
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A document, its history and a runtime to run scripts against them.
pub struct Fixture {
    pub scene: Scene,
    pub history: History,
    pub rt: Runtime,
    _logs: TmpDir,
}

/// A fresh fixture: settings in memory, failures logged in a temp dir.
pub fn fixture() -> Fixture {
    xtrude_model::settings::use_memory_store();
    let logs = TmpDir::new();
    let mut history = History::new();
    history.error_log = Some(logs.0.join("errors.log"));
    Fixture {
        scene: Scene::new(),
        history,
        rt: Runtime::new().expect("Lua runtime"),
        _logs: logs,
    }
}

impl Fixture {
    /// Runs `source` (it gets `doc` as `...`) as the command `label`.
    pub fn run(&mut self, label: &str, source: &str) -> bool {
        let func = self
            .rt
            .load(source, "test.lua")
            .expect("the script compiles");
        let mut cmd = ScriptCommand::new(&self.rt, "test", label, func, Value::Nil);
        self.history.execute(&mut self.scene, &mut cmd)
    }
}
