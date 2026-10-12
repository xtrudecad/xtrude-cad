// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Lua extensions for Xtrude CAD.
//!
//! An extension is a Lua script that reads and edits the document through
//! a small API. This crate holds everything that needs no window: the
//! sandboxed Lua [`Runtime`], its [`ExtError`], the Lua API (`api`), the
//! [`ScriptCommand`] a run executes as, and the extensions on disk
//! ([`discover`], [`Extension`], [`Script`]). The app wires it into its
//! actions and menus.

pub mod api;
pub mod command;
pub mod error;
pub mod extension;
pub mod params;
pub mod runtime;

pub use command::ScriptCommand;
pub use error::{ErrorKind, ExtError};
pub use extension::{default_dir, discover, ExtCommand, Extension, Script};
pub use params::{ParamKind, ParamSpec, ParamValue};
pub use runtime::{Limits, Runtime};
