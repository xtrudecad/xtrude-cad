// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The sandboxed Lua state scripts run in.
//!
//! A script sees only the `table`, `string`, `math` and `utf8` libraries
//! and the safe part of the base library: no `io`, `os`, `package`,
//! `debug`, `dofile` or `loadfile`, and `load` takes source text only,
//! never bytecode. `require` exists only once [`Runtime::set_module_root`]
//! names a folder, and then loads Lua source files from that folder only.
//!
//! The runtime also holds the Lua API (`xtrude`, and the methods of the
//! `doc` a [`crate::ScriptCommand`] passes). Every run is bounded by
//! [`Limits`]: going over the memory limit or the
//! time limit ends the run with an error, which `pcall` cannot swallow.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{
    chunk::ChunkMode, FromLuaMulti, Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib,
    Table, Value, VmState,
};
use xtrude_model::i18n::tr;

use crate::error::{ErrorKind, ExtError};

/// How often, in Lua instructions, the time limit is checked.
const CHECK_EVERY: u32 = 10_000;

/// What one script run may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes the Lua state may allocate.
    pub memory: usize,
    /// Wall-clock time one run may take.
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            memory: 256 << 20,
            time: Duration::from_secs(10),
        }
    }
}

/// The time limit's state, shared with the instruction hook.
#[derive(Default)]
struct Watch {
    /// When the current run must stop; `None` between runs.
    deadline: Cell<Option<Instant>>,
    /// The current run went over its time limit.
    timed_out: Cell<bool>,
}

/// A sandboxed Lua state with its limits.
pub struct Runtime {
    lua: Lua,
    limits: Limits,
    watch: Rc<Watch>,
}

impl Runtime {
    /// A runtime with the default limits (256 MiB, 10 s).
    pub fn new() -> Result<Runtime, ExtError> {
        Runtime::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Result<Runtime, ExtError> {
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(setup_error)?;
        let rt = Runtime {
            lua,
            limits,
            watch: Rc::new(Watch::default()),
        };
        rt.sandbox_globals().map_err(setup_error)?;
        rt.install_time_limit().map_err(setup_error)?;
        crate::api::install(&rt.lua).map_err(setup_error)?;
        rt.lua
            .set_memory_limit(limits.memory)
            .map_err(setup_error)?;
        Ok(rt)
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Lets scripts `require` the Lua modules in `root`: `require "a.b"`
    /// loads `root/a/b.lua` once and returns what it returned. Module names
    /// are dot-separated words, so a script cannot leave the folder.
    pub fn set_module_root(&self, root: impl Into<PathBuf>) -> Result<(), ExtError> {
        let root = root.into();
        let set = || -> mlua::Result<()> {
            let loaded = self.lua.create_table()?;
            let require = self
                .lua
                .create_function(move |lua, name: String| require(lua, &root, &loaded, &name))?;
            self.lua.globals().set("require", require)
        };
        set().map_err(setup_error)
    }

    /// Runs `source` as a chunk named `name` (shown in error messages as
    /// `name:line:`) and converts what it returns.
    pub fn run_chunk<R: FromLuaMulti>(&self, source: &str, name: &str) -> Result<R, ExtError> {
        self.guarded(|lua| {
            lua.load(source)
                .set_name(format!("@{name}"))
                .set_mode(ChunkMode::Text)
                .call(())
        })
    }

    /// Compiles `source` as a chunk named `name` without running it. A
    /// script run as a command receives `doc` and the parameters as `...`.
    pub fn load(&self, source: &str, name: &str) -> Result<Function, ExtError> {
        self.guarded(|lua| {
            lua.load(source)
                .set_name(format!("@{name}"))
                .set_mode(ChunkMode::Text)
                .into_function()
        })
    }

    /// Runs `run` under the time limit and turns a Lua error into an
    /// [`ExtError`].
    pub(crate) fn guarded<R>(
        &self,
        run: impl FnOnce(&Lua) -> mlua::Result<R>,
    ) -> Result<R, ExtError> {
        self.watch.timed_out.set(false);
        self.watch
            .deadline
            .set(Some(Instant::now() + self.limits.time));
        let out = run(&self.lua);
        self.watch.deadline.set(None);
        out.map_err(|e| self.ext_error(&e))
    }

    fn ext_error(&self, e: &mlua::Error) -> ExtError {
        let (message, traceback) = describe(e);
        let (kind, message) = if self.watch.timed_out.get() {
            (ErrorKind::TimeLimit, time_limit_message(self.limits.time))
        } else {
            match innermost(e) {
                mlua::Error::MemoryError(_) => (
                    ErrorKind::Memory,
                    tr(
                        "The script went over its memory limit ({mib} MiB).",
                        &[("mib", &(self.limits.memory >> 20))],
                    ),
                ),
                mlua::Error::SyntaxError { .. } => (ErrorKind::Syntax, message),
                _ => (ErrorKind::Runtime, message),
            }
        };
        ExtError {
            kind,
            message,
            traceback,
        }
    }

    /// Removes what reaches files, and makes `load` refuse bytecode.
    fn sandbox_globals(&self) -> mlua::Result<()> {
        let g = self.lua.globals();
        for name in ["dofile", "loadfile", "require"] {
            g.raw_set(name, Value::Nil)?;
        }
        let raw_load: Function = g.get("load")?;
        let load = self.lua.create_function(move |lua, args: MultiValue| {
            // `load(chunk, chunkname, mode, env)`: force mode "t". An `env`
            // given as nil differs from none, so the arity is kept.
            let mut args: Vec<Value> = args.into_iter().collect();
            if args.len() < 3 {
                args.resize(3, Value::Nil);
            }
            args[2] = Value::String(lua.create_string("t")?);
            raw_load.call::<MultiValue>(MultiValue::from_iter(args))
        })?;
        g.set("load", load)?;
        Ok(())
    }

    /// Aborts a run past its deadline. Once the deadline has passed, every
    /// `pcall` and `xpcall` re-raises, so a script cannot catch the abort
    /// and keep running.
    fn install_time_limit(&self) -> mlua::Result<()> {
        let watch = Rc::clone(&self.watch);
        let time = self.limits.time;
        self.lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(CHECK_EVERY),
            move |_, _| match watch.deadline.get() {
                Some(deadline) if Instant::now() > deadline => {
                    watch.timed_out.set(true);
                    Err(mlua::Error::RuntimeError(time_limit_message(time)))
                }
                _ => Ok(VmState::Continue),
            },
        )?;
        let g = self.lua.globals();
        for name in ["pcall", "xpcall"] {
            let raw: Function = g.get(name)?;
            let watch = Rc::clone(&self.watch);
            let guarded = self.lua.create_function(move |_, args: MultiValue| {
                let out = raw.call::<MultiValue>(args)?;
                if watch.timed_out.get() {
                    return Err(mlua::Error::RuntimeError(time_limit_message(time)));
                }
                Ok(out)
            })?;
            g.set(name, guarded)?;
        }
        Ok(())
    }
}

/// `require` for the modules under `root`.
fn require(lua: &Lua, root: &Path, loaded: &Table, name: &str) -> mlua::Result<Value> {
    match loaded.raw_get::<Value>(name)? {
        Value::Nil => {}
        Value::Boolean(false) => {
            return Err(mlua::Error::RuntimeError(tr(
                "module '{name}' requires itself",
                &[("name", &name)],
            )))
        }
        done => return Ok(done),
    }
    let words: Vec<&str> = name.split('.').collect();
    let valid = |w: &&str| {
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if !words.iter().all(valid) {
        return Err(mlua::Error::RuntimeError(tr(
            "invalid module name '{name}'",
            &[("name", &name)],
        )));
    }
    let rel = format!("{}.lua", words.join("/"));
    let source = std::fs::read_to_string(root.join(&rel)).map_err(|e| {
        mlua::Error::RuntimeError(tr(
            "module '{name}' not found: {error}",
            &[("name", &name), ("error", &e)],
        ))
    })?;
    let chunk_name = match root.file_name() {
        Some(folder) => format!("@{}/{rel}", folder.to_string_lossy()),
        None => format!("@{rel}"),
    };
    loaded.raw_set(name, false)?;
    let module = lua
        .load(source)
        .set_name(chunk_name)
        .set_mode(ChunkMode::Text)
        .call::<Value>(name);
    let module = match module {
        Ok(Value::Nil | Value::Boolean(false)) => Value::Boolean(true),
        Ok(v) => v,
        Err(e) => {
            loaded.raw_set(name, Value::Nil)?;
            return Err(e);
        }
    };
    loaded.raw_set(name, module.clone())?;
    Ok(module)
}

fn time_limit_message(time: Duration) -> String {
    tr(
        "The script ran longer than its time limit ({seconds} s).",
        &[("seconds", &time.as_secs_f64())],
    )
}

fn setup_error(e: mlua::Error) -> ExtError {
    ExtError::new(
        ErrorKind::Runtime,
        tr("Cannot start Lua: {error}", &[("error", &e)]),
    )
}

/// The error a callback or a context wraps.
fn innermost(e: &mlua::Error) -> &mlua::Error {
    match e {
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            innermost(cause)
        }
        _ => e,
    }
}

/// The message and the traceback of a Lua error.
fn describe(e: &mlua::Error) -> (String, Option<String>) {
    match e {
        mlua::Error::SyntaxError { message, .. }
        | mlua::Error::RuntimeError(message)
        | mlua::Error::MemoryError(message) => split_traceback(message),
        mlua::Error::CallbackError { traceback, cause } => {
            let (message, inner) = describe(cause);
            let outer = split_traceback(traceback).1.or_else(|| {
                let tb = traceback.trim();
                (!tb.is_empty()).then(|| tb.to_string())
            });
            (message, inner.or(outer))
        }
        mlua::Error::WithContext { cause, .. } => describe(cause),
        other => (other.to_string(), None),
    }
}

/// Splits Lua's `msg\nstack traceback:\n…` into its two parts.
fn split_traceback(s: &str) -> (String, Option<String>) {
    match s.split_once("stack traceback:\n") {
        Some((message, tb)) => (message.trim_end().to_string(), Some(tb.to_string())),
        None => (s.to_string(), None),
    }
}
