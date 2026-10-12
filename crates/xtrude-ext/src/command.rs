// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A script run as one command: one undo step, rolled back whole when the
//! script fails.

use mlua::{Function, Value};
use xtrude_model::{CmdResult, Command, Scene};

use crate::api::{Host, SaveDialog};
use crate::runtime::Runtime;

/// Calls an extension's Lua function as `func(doc, params)`. Executed
/// through the history, the run is one undo step named after the
/// extension command; a Lua error fails the command, and the history puts
/// the document back as it was.
pub struct ScriptCommand<'rt> {
    runtime: &'rt Runtime,
    /// Answers `xtrude.ask_save_path`; without one, it is always
    /// cancelled.
    save_dialog: Option<Box<SaveDialog<'rt>>>,
    /// The extension the function belongs to.
    pub extension: String,
    name: String,
    func: Function,
    params: Value,
}

impl<'rt> ScriptCommand<'rt> {
    /// `label` is the command's menu label; the undo step drops its
    /// trailing ellipsis ("Spur gear…" → "Spur gear").
    pub fn new(
        runtime: &'rt Runtime,
        extension: &str,
        label: &str,
        func: Function,
        params: Value,
    ) -> ScriptCommand<'rt> {
        let name = label.trim();
        let name = name
            .strip_suffix('…')
            .or_else(|| name.strip_suffix("..."))
            .unwrap_or(name)
            .trim_end();
        ScriptCommand {
            runtime,
            save_dialog: None,
            extension: extension.to_string(),
            name: name.to_string(),
            func,
            params,
        }
    }
}

impl<'rt> ScriptCommand<'rt> {
    /// Lets the script ask the user where to save, through `ask`.
    pub fn with_save_dialog(
        mut self,
        ask: impl FnMut(&str, &str) -> Option<std::path::PathBuf> + 'rt,
    ) -> ScriptCommand<'rt> {
        self.save_dialog = Some(Box::new(ask));
        self
    }
}

impl Command for ScriptCommand<'_> {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let params = self.params.clone();
        let host = Host {
            extension: &self.extension,
            save_dialog: self.save_dialog.as_deref_mut(),
        };
        self.runtime
            .guarded(|lua| crate::api::call_with_doc(lua, scene, &self.func, params, host))?;
        Ok(())
    }
}
