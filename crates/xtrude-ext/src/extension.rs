// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Extensions on disk: discovery, the manifest, and one-off scripts.
//!
//! An extension is a folder `<id>/` whose `init.lua` returns a manifest
//! table: `id` (the folder's name), `name`, `version`, `api` and a list of
//! `commands`, each with an `id`, a `label`, optional `params` and a `run`
//! function. Each extension gets its own [`Runtime`], whose `require`
//! reaches the extension's folder only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mlua::{Function, Table, Value};
use xtrude_model::i18n::tr;

use crate::api::API_VERSION;
use crate::command::ScriptCommand;
use crate::error::{ErrorKind, ExtError};
use crate::params::{params_table, parse_specs, ParamSpec, ParamValue};
use crate::runtime::Runtime;

/// The folder extensions are loaded from: `extensions/` in the app's
/// configuration folder.
pub fn default_dir() -> Option<PathBuf> {
    Some(xtrude_model::settings::config_dir()?.join("extensions"))
}

/// Loads every extension in `dir`, in folder-name order. A folder that
/// fails to load gives its path and the reason, and the others still
/// load. Files, and folders whose name starts with a dot, are not
/// extensions. A missing `dir` holds no extension.
pub fn discover(dir: &Path) -> Vec<Result<Extension, (PathBuf, ExtError)>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            let err = manifest_error(tr(
                "Cannot read the extensions folder: {error}",
                &[("error", &e)],
            ));
            return vec![Err((dir.to_path_buf(), err))];
        }
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
        })
        .collect();
    folders.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    folders
        .into_iter()
        .map(|p| Extension::load(&p).map_err(|e| (p, e)))
        .collect()
}

/// One menu command of an extension.
pub struct ExtCommand {
    pub id: String,
    /// The menu label, as the manifest wrote it.
    pub label: String,
    /// The parameters the user is asked for, in order.
    pub params: Vec<ParamSpec>,
    run: Function,
}

/// A loaded extension: its manifest and the Lua state its commands run in.
pub struct Extension {
    /// The folder's name, and the manifest's `id`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The API version the extension was written for.
    pub api: i64,
    pub dir: PathBuf,
    pub commands: Vec<ExtCommand>,
    runtime: Runtime,
}

impl Extension {
    /// Runs `dir/init.lua` and reads the manifest it returns.
    pub fn load(dir: &Path) -> Result<Extension, ExtError> {
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let source = std::fs::read_to_string(dir.join("init.lua"))
            .map_err(|e| manifest_error(tr("Cannot read init.lua: {error}", &[("error", &e)])))?;
        let runtime = Runtime::new()?;
        runtime.set_module_root(dir)?;
        let manifest: Value = runtime.run_chunk(&source, &format!("{folder}/init.lua"))?;
        let Value::Table(manifest) = manifest else {
            return Err(manifest_error(tr(
                "init.lua must return the manifest table",
                &[],
            )));
        };
        let id = text(&manifest, "id")?;
        if id != folder {
            return Err(manifest_error(tr(
                "The manifest id '{id}' does not match the folder name '{folder}'",
                &[("id", &id), ("folder", &folder)],
            )));
        }
        if !is_word(&id) {
            return Err(manifest_error(tr(
                "The extension id '{id}' may hold only letters, digits, '_' and '-'",
                &[("id", &id)],
            )));
        }
        let api = match manifest.raw_get::<Value>("api").map_err(lua_error)? {
            Value::Integer(n) => n,
            _ => {
                return Err(manifest_error(tr(
                    "The manifest needs 'api', the API version as a whole number",
                    &[],
                )))
            }
        };
        if api > API_VERSION || api < 1 {
            return Err(manifest_error(tr(
                "The extension needs API version {api}; this Xtrude CAD supports version {supported}",
                &[("api", &api), ("supported", &API_VERSION)],
            )));
        }
        let name = text(&manifest, "name")?;
        let version = match manifest.raw_get::<Value>("version").map_err(lua_error)? {
            Value::Nil => String::new(),
            _ => text(&manifest, "version")?,
        };
        let commands = match manifest.raw_get::<Value>("commands").map_err(lua_error)? {
            Value::Nil => Vec::new(),
            Value::Table(list) => commands(&list)?,
            _ => {
                return Err(manifest_error(tr(
                    "'commands' must be a list of commands",
                    &[],
                )))
            }
        };
        Ok(Extension {
            id,
            name,
            version,
            api,
            dir: dir.to_path_buf(),
            commands,
            runtime,
        })
    }

    pub fn command(&self, id: &str) -> Option<&ExtCommand> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// The command `id`, ready to execute with `values`, one per
    /// parameter in order. `None` when the extension has no such command;
    /// an error when a value does not fit its parameter.
    pub fn script_command(
        &self,
        id: &str,
        values: &[ParamValue],
    ) -> Option<Result<ScriptCommand<'_>, ExtError>> {
        let cmd = self.command(id)?;
        Some(cmd.check(values).and_then(|()| {
            let params = params_table(self.runtime.lua(), &cmd.params, values)
                .map_err(|e| ExtError::new(ErrorKind::Runtime, e.to_string()))?;
            Ok(ScriptCommand::new(
                &self.runtime,
                &self.id,
                &cmd.label,
                cmd.run.clone(),
                Value::Table(params),
            ))
        }))
    }
}

impl ExtCommand {
    /// Each parameter's default value.
    pub fn defaults(&self) -> Vec<ParamValue> {
        self.params.iter().map(|p| p.default.clone()).collect()
    }

    /// Whether `values` fit the parameters, one each.
    pub fn check(&self, values: &[ParamValue]) -> Result<(), ExtError> {
        if values.len() != self.params.len() {
            return Err(ExtError::new(
                ErrorKind::Runtime,
                tr(
                    "'{id}' takes {n} parameters, not {given}",
                    &[
                        ("id", &self.id),
                        ("n", &self.params.len()),
                        ("given", &values.len()),
                    ],
                ),
            ));
        }
        for (spec, value) in self.params.iter().zip(values) {
            spec.check(value)
                .map_err(|e| ExtError::new(ErrorKind::Runtime, e))?;
        }
        Ok(())
    }
}

/// A Lua file run once, outside any extension. Its chunk receives `doc`
/// as `...`, and it may `require` modules from its own folder.
pub struct Script {
    /// The file's name, as the undo step shows it.
    pub name: String,
    func: Function,
    runtime: Runtime,
}

impl Script {
    /// Reads and compiles `path`.
    pub fn load(path: &Path) -> Result<Script, ExtError> {
        let source = std::fs::read_to_string(path).map_err(|e| {
            ExtError::new(
                ErrorKind::Runtime,
                tr(
                    "Cannot read {path}: {error}",
                    &[("path", &path.display()), ("error", &e)],
                ),
            )
        })?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let runtime = Runtime::new()?;
        if let Some(folder) = path.parent() {
            runtime.set_module_root(folder)?;
        }
        let func = runtime.load(&source, &name)?;
        Ok(Script {
            name,
            func,
            runtime,
        })
    }

    /// The run, as a command named `label`.
    pub fn command(&self, label: &str) -> ScriptCommand<'_> {
        ScriptCommand::new(&self.runtime, "", label, self.func.clone(), Value::Nil)
    }
}

/// The manifest's `commands` list.
fn commands(list: &Table) -> Result<Vec<ExtCommand>, ExtError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in list.sequence_values::<Value>() {
        let Value::Table(entry) = entry.map_err(lua_error)? else {
            return Err(manifest_error(tr("Each command must be a table", &[])));
        };
        let id = text(&entry, "id")?;
        if !is_word(&id) {
            return Err(manifest_error(tr(
                "The command id '{id}' may hold only letters, digits, '_' and '-'",
                &[("id", &id)],
            )));
        }
        if !seen.insert(id.clone()) {
            return Err(manifest_error(tr(
                "Two commands have the id '{id}'",
                &[("id", &id)],
            )));
        }
        let label = text(&entry, "label")?;
        let Value::Function(run) = entry.raw_get::<Value>("run").map_err(lua_error)? else {
            return Err(manifest_error(tr(
                "The command '{id}' needs a 'run' function",
                &[("id", &id)],
            )));
        };
        let params = match entry.raw_get::<Value>("params").map_err(lua_error)? {
            Value::Nil => Vec::new(),
            Value::Table(t) => parse_specs(&id, &t)?,
            _ => {
                return Err(manifest_error(tr(
                    "The 'params' of command '{id}' must be a list",
                    &[("id", &id)],
                )))
            }
        };
        out.push(ExtCommand {
            id,
            label,
            params,
            run,
        });
    }
    Ok(out)
}

/// The non-empty string field `key` of `table`.
fn text(table: &Table, key: &str) -> Result<String, ExtError> {
    match table.raw_get::<Value>(key).map_err(lua_error)? {
        Value::String(s) => {
            let s = s.to_string_lossy();
            if s.trim().is_empty() {
                Err(missing(key))
            } else {
                Ok(s)
            }
        }
        _ => Err(missing(key)),
    }
}

fn missing(key: &str) -> ExtError {
    manifest_error(tr(
        "The manifest needs '{key}' as a non-empty string",
        &[("key", &key)],
    ))
}

/// Letters, digits, `_` and `-`: safe in an action id and a folder name.
fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn manifest_error(message: String) -> ExtError {
    ExtError::new(ErrorKind::Manifest, message)
}

fn lua_error(e: mlua::Error) -> ExtError {
    manifest_error(e.to_string())
}
