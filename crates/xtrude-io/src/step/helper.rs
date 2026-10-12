// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The backend that runs the `xtrude-step` helper program.
//!
//! OpenCascade lives in that separate process, so a file that crashes it,
//! hangs it or exhausts its memory only ends the helper: the import fails
//! with an error and the document is untouched. Cancel kills it.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::protocol::{self, RawStep, EXIT_NO_OCCT, PROGRESS_PREFIX};
use super::{StepBackend, StepError, Tessellation};
use crate::dae::Progress;

/// The environment variable that names the helper program, over the
/// default (`xtrude-step` next to the running program).
pub const HELPER_ENV: &str = "XTRUDE_STEP_HELPER";

/// The helper's file name on this platform.
pub fn helper_file_name() -> String {
    format!("xtrude-step{}", std::env::consts::EXE_SUFFIX)
}

/// Reads STEP through the helper program at `program`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperBackend {
    pub program: PathBuf,
}

impl HelperBackend {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        HelperBackend {
            program: program.into(),
        }
    }

    /// The helper named by [`HELPER_ENV`], or the one next to the running
    /// program. It may not exist: [`StepBackend::parse`] then reports
    /// [`StepError::NotInstalled`].
    pub fn locate() -> Self {
        if let Some(path) = std::env::var_os(HELPER_ENV).filter(|p| !p.is_empty()) {
            return HelperBackend::new(path);
        }
        let dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        HelperBackend::new(dir.join(helper_file_name()))
    }
}

impl StepBackend for HelperBackend {
    fn name(&self) -> &str {
        "opencascade"
    }

    fn parse(
        &self,
        path: &Path,
        tessellation: &Tessellation,
        mut progress: Option<Progress<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> Result<RawStep, StepError> {
        let spawned = Command::new(&self.program)
            .arg(path)
            .arg("--chord-mm")
            .arg((tessellation.chord * 1000.0).to_string())
            .arg("--angle-deg")
            .arg(tessellation.angle_deg.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StepError::NotInstalled)
            }
            Err(e) => return Err(StepError::Io(e.to_string())),
        };
        let (Some(mut stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            return Err(StepError::Io("no pipe to the STEP helper".into()));
        };
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            stdout.read_to_end(&mut out).map(|_| out)
        });
        let (lines_tx, lines) = mpsc::channel::<String>();
        let errors = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if lines_tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut message = Vec::new();
        let status = loop {
            match lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => match line.strip_prefix(PROGRESS_PREFIX) {
                    Some(rest) => {
                        let (fraction, text) = rest.split_once(' ').unwrap_or((rest, ""));
                        if let (Some(p), Ok(f)) = (progress.as_mut(), fraction.parse::<f64>()) {
                            p(f.clamp(0.0, 1.0), text);
                        }
                    }
                    None => message.push(line),
                },
                // Every stderr line is in: the helper is finishing.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break child.wait().map_err(|e| StepError::Io(e.to_string()))?
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                // The pipe threads end with the pipes; not waiting for them
                // keeps Cancel immediate.
                let _ = child.kill();
                let _ = child.wait();
                return Err(StepError::Cancelled);
            }
        };
        let _ = errors.join();
        let out = reader
            .join()
            .map_err(|_| StepError::Io("the STEP helper reader failed".into()))?
            .map_err(|e| StepError::Io(e.to_string()))?;
        match status.code() {
            Some(0) => protocol::decode(&out).map_err(|e| StepError::BadOutput(e.0)),
            Some(EXIT_NO_OCCT) => Err(StepError::NotInstalled),
            Some(_) => Err(StepError::Read(message.join("\n").trim().to_string())),
            // Ended by a signal: it crashed or was killed.
            None => Err(StepError::Crashed),
        }
    }
}
