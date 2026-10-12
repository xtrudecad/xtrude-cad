// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What is drawing the viewport, for the About box and the log folder.
//!
//! A "terribly slow" report has one question behind it: is the viewport on
//! the GPU or on a software rasteriser? The shell hands the window the
//! adapter it got; the window keeps it for the About box and writes
//! `xtrude-gl.txt` in the log folder.

use std::path::{Path, PathBuf};

use xtrude_model::i18n::tr;
use xtrude_render::gpuinfo::{describe, write_gl_report, GpuInfo};

use super::MainWindow;

impl MainWindow {
    /// Keep the device the viewport draws on and write it to
    /// `xtrude-gl.txt` (in the log folder, or at `report` when given);
    /// the report's path, or `None` when nothing is writable.
    pub fn set_gpu_info(&mut self, info: GpuInfo, report: Option<&Path>) -> Option<PathBuf> {
        let path = write_gl_report(&info, report);
        self.gpu = Some(info);
        path
    }

    /// The About box's line for the drawing device, `None` before the
    /// shell has one or when the driver names nothing.
    pub fn gpu_line(&self) -> Option<String> {
        let info = self.gpu.as_ref()?;
        let device = describe(info);
        if device.is_empty() {
            return None;
        }
        let mut line = tr("Graphics: {device}", &[("device", &device)]);
        if info.software {
            line.push_str(&format!(" — {}", tr("software rendering, slow", &[])));
        }
        Some(line)
    }
}
