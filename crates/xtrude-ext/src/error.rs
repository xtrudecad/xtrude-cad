// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The error a script run ends with.

use xtrude_model::CmdError;

/// Why a script run failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The source does not parse.
    Syntax,
    /// The script raised an error (`error(…)`, a bad call, …).
    Runtime,
    /// The script went over the memory limit.
    Memory,
    /// The script ran longer than the time limit.
    TimeLimit,
    /// An extension's folder or manifest is not usable.
    Manifest,
}

/// A failed script run: what went wrong, with the Lua traceback when
/// there is one. `message` names the chunk and line (`gears/init.lua:12:
/// …`) for syntax and runtime errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ExtError {
    pub kind: ErrorKind,
    pub message: String,
    /// Lua's `stack traceback:` lines, without that heading.
    pub traceback: Option<String>,
}

impl ExtError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> ExtError {
        ExtError {
            kind,
            message: message.into(),
            traceback: None,
        }
    }

    /// The message followed by the traceback, as shown to the user.
    pub fn report(&self) -> String {
        match &self.traceback {
            Some(tb) => format!("{}\nstack traceback:\n{tb}", self.message),
            None => self.message.clone(),
        }
    }
}

/// A script run is a command's body: its failure fails the command, and
/// the history rolls the document back.
impl From<ExtError> for CmdError {
    fn from(e: ExtError) -> CmdError {
        CmdError::Failed(e.report())
    }
}
