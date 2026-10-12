// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The modal progress of a long import.
//!
//! A worker parses while the dialog keeps painting. An immediate-mode UI
//! cannot nest its event loop, so the worker is an [`ImportJob`] the
//! window keeps and the shell
//! polls once a frame; its milestones land in an [`ImportProgress`], the
//! dialog's model. Tests (and scripts) wait for the job instead. A job
//! that can stop early (the STEP import, which runs a helper program)
//! carries a cancel flag, and the dialog then offers Cancel.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use xtrude_io::step::{self, HelperBackend, StepError, StepPayload, Tessellation};
use xtrude_io::{skp, stl};
use xtrude_model::i18n::tr;

/// How long an import runs before its dialog appears
/// (`setMinimumDuration(400)`): small ones never show it.
pub const MINIMUM_DURATION: Duration = Duration::from_millis(400);

/// The progress dialog's model: a 0–100 bar and a label, not closed by
/// reaching 100. Cancel shows only for a job that can stop
/// ([`ImportJob::can_cancel`]).
#[derive(Clone, Debug)]
pub struct ImportProgress {
    /// The bar, 0–100.
    pub value: i32,
    /// The text above the bar: first the dialog's title, then the
    /// loader's latest milestone.
    pub label: String,
    started: Instant,
}

impl ImportProgress {
    pub fn new(title: &str) -> ImportProgress {
        ImportProgress {
            value: 0,
            label: title.to_string(),
            started: Instant::now(),
        }
    }

    /// The loaders' callback `cb(frac, text)`: the bar to `int(frac * 100)`,
    /// the label to the translated milestone.
    pub fn report(&mut self, fraction: f64, text: &str) {
        self.value = (fraction * 100.0) as i32;
        self.label = tr(text, &[]);
    }

    /// Whether the dialog is on screen at `now`: only once the import has
    /// run [`MINIMUM_DURATION`].
    pub fn shown_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= MINIMUM_DURATION
    }

    pub fn is_shown(&self) -> bool {
        self.shown_at(Instant::now())
    }
}

/// Why a threaded `.skp` parse produced no payload.
pub enum SkpParse {
    /// No backend could read the file.
    NeedsConverter,
    /// The worker itself failed.
    Failed(String),
}

/// What a worker hands back.
pub enum ImportResult {
    Skp(Result<skp::SkpPayload, SkpParse>),
    Stl(Result<xtrude_core::Mesh, stl::StlError>),
    Step(Result<StepPayload, StepError>),
}

enum Msg {
    Progress(f64, String),
    Done(Box<ImportResult>),
}

/// The parse's thread stack: the classic `.skp` reader recurses once per
/// nested record.
const SKP_STACK: usize = 64 * 1024 * 1024;

/// A parse running on a worker thread. It touches no `Scene`: the window
/// applies its result on the UI thread, as one undo step.
pub struct ImportJob {
    /// The file being imported.
    pub path: PathBuf,
    rx: Receiver<Msg>,
    worker: Option<JoinHandle<()>>,
    /// The result of a worker that died without one.
    lost: fn() -> ImportResult,
    /// Set to ask the worker to stop; `None` for a job that cannot.
    cancel: Option<Arc<AtomicBool>>,
}

impl ImportJob {
    fn spawn(
        path: &Path,
        builder: std::thread::Builder,
        lost: fn() -> ImportResult,
        parse: impl FnOnce(&Path, &mut dyn FnMut(f64, &str)) -> ImportResult + Send + 'static,
    ) -> std::io::Result<ImportJob> {
        let (tx, rx) = mpsc::channel();
        let worker_path = path.to_path_buf();
        let worker = builder.spawn(move || {
            let tx_progress = tx.clone();
            let mut report = move |f: f64, t: &str| {
                let _ = tx_progress.send(Msg::Progress(f, t.to_string()));
            };
            let result = parse(&worker_path, &mut report);
            let _ = tx.send(Msg::Done(Box::new(result)));
        })?;
        Ok(ImportJob {
            path: path.to_path_buf(),
            rx,
            worker: Some(worker),
            lost,
            cancel: None,
        })
    }

    /// Parse a `.skp` on a worker.
    pub fn skp(path: &Path) -> std::io::Result<ImportJob> {
        let builder = std::thread::Builder::new()
            .name("skp-parse".into())
            .stack_size(SKP_STACK);
        let lost = || {
            ImportResult::Skp(Err(SkpParse::Failed(
                "the .skp reader stopped unexpectedly".into(),
            )))
        };
        ImportJob::spawn(path, builder, lost, |p, report| {
            ImportResult::Skp(skp::parse_skp(p, Some(report)).map_err(|_| SkpParse::NeedsConverter))
        })
    }

    /// Parse an STL on a worker.
    pub fn stl(path: &Path, scale: f64, mode: stl::SimplifyMode) -> std::io::Result<ImportJob> {
        let builder = std::thread::Builder::new().name("stl-parse".into());
        let lost = || {
            ImportResult::Stl(Err(stl::StlError::Io(std::io::Error::other(
                "the STL reader stopped unexpectedly",
            ))))
        };
        ImportJob::spawn(path, builder, lost, move |p, report| {
            ImportResult::Stl(stl::parse_stl(p, Some(report), scale, mode))
        })
    }

    /// Read a STEP file on a worker with the helper program `helper`.
    pub fn step(
        path: &Path,
        helper: HelperBackend,
        tessellation: Tessellation,
    ) -> std::io::Result<ImportJob> {
        let builder = std::thread::Builder::new().name("step-parse".into());
        let lost = || {
            ImportResult::Step(Err(StepError::Io(
                "the STEP import stopped unexpectedly".into(),
            )))
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let mut job = ImportJob::spawn(path, builder, lost, move |p, report| {
            ImportResult::Step(step::parse_step_with(
                &helper,
                p,
                &tessellation,
                Some(report),
                Some(&flag),
            ))
        })?;
        job.cancel = Some(cancel);
        Ok(job)
    }

    /// Whether the job can stop early (the dialog then offers Cancel).
    pub fn can_cancel(&self) -> bool {
        self.cancel.is_some()
    }

    /// Ask the worker to stop; its result then says it was cancelled.
    /// Nothing for a job that cannot stop.
    pub fn cancel(&self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// Whether Cancel was asked for.
    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// Deliver the progress received so far to `cb`, on the calling
    /// thread; the result once the worker is done.
    pub fn poll(&mut self, cb: &mut dyn FnMut(f64, &str)) -> Option<ImportResult> {
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Progress(f, t)) => cb(f, &t),
                Ok(Msg::Done(r)) => return Some(self.finish(Some(*r))),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => return Some(self.finish(None)),
            }
        }
    }

    /// Block until the worker is done, delivering its progress to `cb` on
    /// the calling thread.
    pub fn wait(mut self, cb: &mut dyn FnMut(f64, &str)) -> ImportResult {
        let mut result = None;
        for msg in self.rx.iter() {
            match msg {
                Msg::Progress(f, t) => cb(f, &t),
                Msg::Done(r) => {
                    result = Some(*r);
                    break;
                }
            }
        }
        self.finish(result)
    }

    /// Join the worker; one that died without a result failed.
    fn finish(&mut self, result: Option<ImportResult>) -> ImportResult {
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        result.unwrap_or_else(self.lost)
    }
}
