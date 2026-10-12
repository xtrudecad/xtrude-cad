// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Extensions menu: one action per extension command
//! (`ext.<extension id>.<command id>`), and the fixed entries to run a Lua
//! file once, reload the extensions and open their folder.
//!
//! The window loads nothing until it is given a folder
//! ([`MainWindow::load_extensions`]): the app passes
//! [`xtrude_ext::default_dir`] at start-up, tests a temporary folder.

use std::path::PathBuf;

use xtrude_ext::{ExtError, Extension, ParamSpec, ParamValue, Script, ScriptCommand};
use xtrude_model::i18n::tr;
use xtrude_model::settings::{self, keys, JsonText};
use xtrude_model::units;

use super::actions::plain;
use super::{filedialogs, Action, Dialogs, MainWindow, MenuItem};
use crate::viewport::Viewport;

/// The prefix of an extension command's action id.
const PREFIX: &str = "ext.";
pub const RUN_SCRIPT: &str = "ext_run_script";
pub const RELOAD: &str = "ext_reload";
pub const OPEN_FOLDER: &str = "ext_open_folder";
/// The disabled «N extensions failed to load» entry.
pub const FAILED: &str = "ext_failed";
/// Lists each extension that failed to load, with the reason.
pub const SHOW_FAILURES: &str = "ext_failures";

const LUA_FILE_FILTER: &str = "Lua script (*.lua);;All files (*)";

/// The extensions the window loaded, and those it could not load.
#[derive(Default)]
pub struct Extensions {
    /// Where they are loaded from (`None`: nowhere, nothing is loaded).
    pub dir: Option<PathBuf>,
    pub loaded: Vec<Extension>,
    /// Each folder that failed to load, with the reason.
    pub failed: Vec<(PathBuf, ExtError)>,
}

/// The action id of an extension's command.
pub fn action_id(ext: &str, command: &str) -> String {
    format!("{PREFIX}{ext}.{command}")
}

/// The Extensions menu's title.
pub fn menu_title() -> String {
    tr("Extensions", &[])
}

/// The values `stored` holds for `specs`, each one still valid, else the
/// parameter's default.
fn remembered(specs: &[ParamSpec], stored: &serde_json::Value, scale: f64) -> Vec<ParamValue> {
    specs
        .iter()
        .map(|spec| {
            stored
                .get(&spec.key)
                .and_then(|v| spec.from_json(v, scale))
                .unwrap_or_else(|| spec.default.clone())
        })
        .collect()
}

/// Executes a script run as one undo step; `xtrude.ask_save_path` asks
/// through `dialogs`, starting where the file dialogs last were.
fn execute_script(
    viewport: &mut Viewport,
    dialogs: &mut dyn Dialogs,
    folder: Option<PathBuf>,
    cmd: ScriptCommand<'_>,
) -> bool {
    let ask = move |title: &str, filter: &str| {
        let start = filedialogs::start_in("", folder.as_deref());
        let picked = dialogs.save_file(title, filter, &start);
        filedialogs::remember(picked.as_deref());
        picked
    };
    viewport.execute(&mut cmd.with_save_dialog(ask))
}

impl MainWindow {
    /// Loads the extensions in `dir`, replacing those loaded before.
    pub fn load_extensions(&mut self, dir: Option<PathBuf>) {
        self.extensions.dir = dir;
        self.reload_extensions();
    }

    /// Drops the extension actions and loads the folder again.
    pub fn reload_extensions(&mut self) {
        self.extensions.loaded.clear();
        self.extensions.failed.clear();
        if let Some(dir) = self.extensions.dir.clone() {
            for found in xtrude_ext::discover(&dir) {
                match found {
                    Ok(ext) => self.extensions.loaded.push(ext),
                    Err(failure) => self.extensions.failed.push(failure),
                }
            }
        }
        self.actions.retain(|a| !a.id.starts_with(PREFIX));
        for ext in &self.extensions.loaded {
            for cmd in &ext.commands {
                self.actions.add(
                    Action::new(&action_id(&ext.id, &cmd.id), &cmd.label)
                        .tip(&tr("From the {name} extension.", &[("name", &ext.name)])),
                );
            }
        }
        self.add_extension_fixed_actions();
        let title = menu_title();
        let menu = self.extensions_menu();
        if let Some((_, items)) = self.menus.iter_mut().find(|(t, _)| *t == title) {
            *items = menu;
        }
    }

    fn add_extension_fixed_actions(&mut self) {
        let failed = self.extensions.failed.len();
        let a = &mut self.actions;
        a.add(
            Action::new(RUN_SCRIPT, &tr("Run Lua Script…", &[])).tip(&tr(
                "Run a Lua file once against the document, as one undo step.",
                &[],
            )),
        );
        a.add(
            Action::new(RELOAD, &tr("Reload Extensions", &[]))
                .tip(&tr("Load the extensions from their folder again.", &[])),
        );
        a.add(
            Action::new(OPEN_FOLDER, &tr("Open Extensions Folder", &[]))
                .tip(&tr("Show the folder the extensions are loaded from.", &[])),
        );
        let text = if failed == 1 {
            tr("1 extension failed to load", &[])
        } else {
            tr("{n} extensions failed to load", &[("n", &failed)])
        };
        let mut note = Action::new(FAILED, &text);
        note.enabled = false;
        a.add(note);
        a.add(
            Action::new(SHOW_FAILURES, &tr("Show Loading Errors…", &[])).tip(&tr(
                "List the extensions that failed to load, and why.",
                &[],
            )),
        );
    }

    /// The Extensions menu: each extension's commands (a submenu for an
    /// extension with several), then the fixed entries.
    pub fn extensions_menu(&self) -> Vec<MenuItem> {
        use MenuItem::{Action as A, Menu as M, Separator as S};
        let mut items = Vec::new();
        for ext in &self.extensions.loaded {
            let acts: Vec<MenuItem> = ext
                .commands
                .iter()
                .map(|c| A(action_id(&ext.id, &c.id)))
                .collect();
            match acts.len() {
                0 => {}
                1 => items.extend(acts),
                _ => items.push(M(ext.name.clone(), acts)),
            }
        }
        if !items.is_empty() {
            items.push(S);
        }
        items.extend([
            A(RUN_SCRIPT.into()),
            A(RELOAD.into()),
            A(OPEN_FOLDER.into()),
        ]);
        if !self.extensions.failed.is_empty() {
            items.extend([S, A(FAILED.into()), A(SHOW_FAILURES.into())]);
        }
        items
    }

    /// Runs the Extensions menu's action `id`; `false` if it is not one.
    pub(super) fn dispatch_extensions(&mut self, id: &str) -> bool {
        if let Some(rest) = id.strip_prefix(PREFIX) {
            if let Some((ext, cmd)) = rest.split_once('.') {
                self.run_extension_command(ext, cmd);
            }
            return true;
        }
        match id {
            RUN_SCRIPT => self.run_lua_script(),
            RELOAD => self.reload_extensions(),
            OPEN_FOLDER => self.open_extensions_folder(),
            SHOW_FAILURES => self.show_extension_failures(),
            FAILED => {}
            _ => return false,
        }
        true
    }

    fn run_extension_command(&mut self, ext_id: &str, cmd_id: &str) {
        let Some(ext) = self.extensions.loaded.iter().find(|e| e.id == ext_id) else {
            return;
        };
        let Some(cmd) = ext.command(cmd_id) else {
            return;
        };
        let title = tr("Extension {name}", &[("name", &ext.name)]);
        let mut values = Vec::new();
        if !cmd.params.is_empty() {
            // lengths are kept in metres, so they outlive a change of unit
            let scale = units::bare_scale_of(&self.viewport.scene.units.length).unwrap_or(1.0);
            let key = keys::ext_params(&ext.id, &cmd.id);
            let offered = remembered(&cmd.params, &settings::read(&key).0, scale);
            let Some(given) = self
                .dialogs
                .ask_form(&plain(&cmd.label), &cmd.params, &offered)
            else {
                return;
            };
            if let Err(e) = cmd.check(&given) {
                self.dialogs.report(&title, &e.report());
                return;
            }
            let stored: serde_json::Map<String, serde_json::Value> = cmd
                .params
                .iter()
                .zip(&given)
                .map(|(spec, v)| (spec.key.clone(), spec.to_json(v, scale)))
                .collect();
            settings::write(&key, &JsonText(stored.into()));
            values = given;
        }
        let ok = match ext.script_command(cmd_id, &values) {
            None => return,
            Some(Ok(cmd)) => {
                let folder = self.document_folder();
                execute_script(&mut self.viewport, &mut *self.dialogs, folder, cmd)
            }
            Some(Err(e)) => {
                self.dialogs.report(&title, &e.report());
                return;
            }
        };
        self.after_script(ok, &title);
    }

    /// Run Lua Script…: a `.lua` file the user picks, run once.
    fn run_lua_script(&mut self) {
        let Some(path) = self.pick_open(&tr("Run Lua Script", &[]), &tr(LUA_FILE_FILTER, &[]))
        else {
            return;
        };
        let title = tr("Run Lua Script", &[]);
        let script = match Script::load(&path) {
            Ok(s) => s,
            Err(e) => {
                self.dialogs.report(&title, &e.report());
                return;
            }
        };
        let label = tr("Run {name}", &[("name", &script.name)]);
        let folder = self.document_folder();
        let cmd = script.command(&label);
        let ok = execute_script(&mut self.viewport, &mut *self.dialogs, folder, cmd);
        self.after_script(ok, &title);
    }

    /// Shows the scene a script changed, or why it failed (the history has
    /// put the document back).
    fn after_script(&mut self, ok: bool, title: &str) {
        if !ok {
            let err = self.viewport.history.last_error.clone().unwrap_or_default();
            self.dialogs.report(title, &err);
        }
        self.viewport.notify_scene_changed();
    }

    /// Creates the extensions folder if needed and asks the shell to show
    /// it.
    fn open_extensions_folder(&mut self) {
        let Some(dir) = self.extensions.dir.clone() else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.dialogs.warn(
                &tr("Open Extensions Folder", &[]),
                &tr(
                    "Cannot create {path}: {error}",
                    &[
                        ("path", &dir.display().to_string()),
                        ("error", &e.to_string()),
                    ],
                ),
            );
            return;
        }
        self.open_folder_requested = Some(dir);
    }

    fn show_extension_failures(&mut self) {
        let text: Vec<String> = self
            .extensions
            .failed
            .iter()
            .map(|(path, e)| format!("{}\n{}", path.display(), e.report()))
            .collect();
        self.dialogs.report(
            &tr("Extensions that failed to load", &[]),
            &text.join("\n\n"),
        );
    }
}
