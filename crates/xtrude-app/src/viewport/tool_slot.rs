// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The active tool's slot.

use crate::tools::Tool;

/// A tool switch asked for while the active tool was out of its slot.
#[derive(Default)]
pub(super) enum ToolSwitch {
    /// No switch was asked for: the tool goes back to its slot.
    #[default]
    NotRequested,
    /// Leave the tool with no tool active.
    ToNoTool,
    /// Activate this tool.
    To(Box<dyn Tool>),
}

impl ToolSwitch {
    /// The request to activate `tool` (`None` = no tool).
    pub(super) fn to(tool: Option<Box<dyn Tool>>) -> ToolSwitch {
        match tool {
            Some(t) => ToolSwitch::To(t),
            None => ToolSwitch::ToNoTool,
        }
    }
}

/// The active tool and its lending: while one of the tool's callbacks
/// runs ([`super::Viewport::with_tool`]) the tool is out of its slot
/// (`out`, and `active` is empty), and a tool switch or a cursor change it
/// asks for meanwhile waits in `pending` / `cursor_dirty` until it is back.
#[derive(Default)]
pub(crate) struct ToolSlot {
    pub(super) active: Option<Box<dyn Tool>>,
    /// The active tool is out of its slot while one of its callbacks runs.
    pub(super) out: bool,
    pub(super) pending: ToolSwitch,
    /// The tool's pointer changed while it was out: re-read it on return.
    pub(crate) cursor_dirty: bool,
}
