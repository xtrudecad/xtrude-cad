// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STEP import (`.step`, `.stp`; AP203, AP214, AP242): the seam between
//! Xtrude CAD and how a STEP file is read.
//!
//! **Parse then apply**, as for `.skp`: [`parse_step`] turns the file
//! into a [`StepPayload`] without touching a `Scene`, and
//! [`apply_payload`] adds it inside one undo step, so a failed parse never
//! leaves a half-applied edit.
//!
//! STEP describes exact surfaces; the kernel is a mesh, so the file is
//! tessellated on import ([`Tessellation`]: a chord and an angle
//! tolerance) and becomes ordinary faces. Each part becomes a group with
//! its name, placement and colours; assemblies become nested groups, and
//! a part placed several times is a component whose instances share one
//! mesh. Lengths are converted from the file's unit to metres.
//!
//! The one backend ([`helper::HelperBackend`]) runs OpenCascade in the
//! separate `xtrude-step` program; [`StepBackend`] keeps it replaceable.

pub mod helper;
pub mod protocol;

mod convert;

use std::fmt;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};
use xtrude_model::i18n::tr;
use xtrude_model::scene::Scene;

use crate::dae::Progress;
use crate::skp::SkpPayload;

pub use helper::HelperBackend;
pub use protocol::RawStep;

/// The file extensions read as STEP (lower case).
pub const EXTENSIONS: &[&str] = &["step", "stp"];

/// Whether `path` has a STEP extension.
pub fn is_step_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

// ---- Tessellation ----------------------------------------------------------------

/// How finely curved surfaces become flat faces: no face strays more than
/// `chord` from the exact surface, and neighbouring faces turn by at most
/// `angle_deg`. The defaults suit printing: a 0.01 mm chord gives a Ø20
/// hole about 70 sides, and 10° gives every circle at least 36.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tessellation {
    /// The chord tolerance, in metres.
    pub chord: f64,
    /// The angular tolerance, in degrees.
    pub angle_deg: f64,
}

impl Tessellation {
    /// The chord range accepted from settings, in millimetres.
    pub const CHORD_MM_RANGE: (f64, f64) = (0.001, 1.0);
    /// The angle range accepted from settings, in degrees.
    pub const ANGLE_DEG_RANGE: (f64, f64) = (1.0, 45.0);

    /// The tolerances stored in the `import/step_tessellation` setting
    /// (`{"chord_mm": 0.01, "angle_deg": 10}`); a missing, non-numeric or
    /// out-of-range value takes its default.
    pub fn from_json(value: &Value) -> Self {
        let d = Tessellation::default();
        let read = |name: &str, (lo, hi): (f64, f64)| {
            value
                .get(name)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (lo..=hi).contains(v))
        };
        Tessellation {
            chord: read("chord_mm", Self::CHORD_MM_RANGE).map_or(d.chord, |mm| mm / 1000.0),
            angle_deg: read("angle_deg", Self::ANGLE_DEG_RANGE).unwrap_or(d.angle_deg),
        }
    }

    /// The setting's value for these tolerances.
    pub fn to_json(&self) -> Value {
        json!({ "chord_mm": self.chord * 1000.0, "angle_deg": self.angle_deg })
    }
}

impl Default for Tessellation {
    fn default() -> Self {
        Tessellation {
            chord: 0.01e-3,
            angle_deg: 10.0,
        }
    }
}

// ---- Errors and warnings ---------------------------------------------------------

/// Why a STEP file could not be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepError {
    /// No STEP reader is installed (no helper program, or one built
    /// without OpenCascade).
    NotInstalled,
    /// The reader could not read the file; its message.
    Read(String),
    /// The reader crashed on the file.
    Crashed,
    /// The reader's output could not be understood.
    BadOutput(String),
    /// The file holds no geometry.
    NoGeometry,
    /// The user cancelled.
    Cancelled,
    /// Starting or talking to the reader failed.
    Io(String),
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            StepError::NotInstalled => tr("STEP support is not installed.", &[]),
            StepError::Read(msg) if msg.is_empty() => tr("The STEP file could not be read.", &[]),
            StepError::Read(msg) => tr("The STEP file could not be read: {msg}", &[("msg", msg)]),
            StepError::Crashed => tr("The STEP reader stopped on this file.", &[]),
            StepError::BadOutput(msg) => tr(
                "The STEP reader gave an unreadable result: {msg}",
                &[("msg", msg)],
            ),
            StepError::NoGeometry => tr("The STEP file holds no geometry.", &[]),
            StepError::Cancelled => tr("The STEP import was cancelled.", &[]),
            StepError::Io(msg) => tr("The STEP reader could not run: {msg}", &[("msg", msg)]),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for StepError {}

/// Something the import skipped or could not make whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepWarning {
    /// A part's shell is not closed: it imports as faces, not a solid.
    OpenShell { part: String },
    /// Faces of a part the reader could not tessellate.
    FacesNotMeshed { part: String, count: usize },
    /// Faces smaller than the weld tolerance (about 0.1 mm), dropped.
    SmallFeatures { count: usize },
    /// A closed part with a feature thinner than the weld tolerance: once
    /// welded it is no longer closed, so it imports as faces.
    ThinFeatures { part: String },
}

impl fmt::Display for StepWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            StepWarning::OpenShell { part } => tr(
                "“{part}” is not closed: it imports as faces, not as a solid.",
                &[("part", part)],
            ),
            StepWarning::FacesNotMeshed { part, count } => tr(
                "{count} faces of “{part}” could not be tessellated and are missing.",
                &[("count", count), ("part", part)],
            ),
            StepWarning::ThinFeatures { part } => tr(
                "“{part}” has features thinner than 0.1 mm: it imports as faces, not as a solid.",
                &[("part", part)],
            ),
            StepWarning::SmallFeatures { count } => tr(
                "{count} faces smaller than 0.1 mm were left out.",
                &[("count", count)],
            ),
        };
        f.write_str(&text)
    }
}

// ---- The payload and the seam ----------------------------------------------------

/// A parsed STEP file, ready for [`apply_payload`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepPayload {
    /// The geometry, in the payload the `.skp` import applies too: one
    /// prototype per part (faces in the part's frame, children placed by
    /// matrix) and the top-level placements.
    pub model: SkpPayload,
    pub warnings: Vec<StepWarning>,
}

impl StepPayload {
    /// The payload for a helper's output; [`StepError::NoGeometry`] when
    /// no part has faces.
    pub fn from_raw(raw: &RawStep) -> Result<Self, StepError> {
        let (model, warnings) = convert::convert(raw).ok_or(StepError::NoGeometry)?;
        Ok(StepPayload { model, warnings })
    }
}

/// A way of reading STEP into meshes.
pub trait StepBackend {
    fn name(&self) -> &str;
    /// The meshed file. `cancel`, when set, stops the read with
    /// [`StepError::Cancelled`].
    fn parse(
        &self,
        path: &Path,
        tessellation: &Tessellation,
        progress: Option<Progress<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> Result<RawStep, StepError>;
}

/// `parse_step`: read `path` with the installed helper. Touches no
/// `Scene`; safe on a worker thread.
pub fn parse_step(
    path: &Path,
    tessellation: &Tessellation,
    progress: Option<Progress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<StepPayload, StepError> {
    parse_step_with(
        &HelperBackend::locate(),
        path,
        tessellation,
        progress,
        cancel,
    )
}

/// [`parse_step`] with an explicit backend.
pub fn parse_step_with(
    backend: &dyn StepBackend,
    path: &Path,
    tessellation: &Tessellation,
    progress: Option<Progress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<StepPayload, StepError> {
    let raw = backend.parse(path, tessellation, progress, cancel)?;
    StepPayload::from_raw(&raw)
}

/// `apply_payload`: add a parsed file to `scene`, one group per top-level
/// part or assembly. The caller wraps it in one undo step.
pub fn apply_payload(scene: &mut Scene, payload: &StepPayload) -> Result<(), String> {
    crate::skp::apply_payload(scene, &payload.model).map(|_| ())
}

/// `load_step`: [`parse_step`] then [`apply_payload`]; returns the
/// warnings. The app splits the two so the parse runs outside the undo
/// history.
pub fn load_step(
    scene: &mut Scene,
    path: &Path,
    tessellation: &Tessellation,
    progress: Option<Progress<'_>>,
) -> Result<Vec<StepWarning>, String> {
    let payload = parse_step(path, tessellation, progress, None).map_err(|e| e.to_string())?;
    apply_payload(scene, &payload)?;
    Ok(payload.warnings)
}
