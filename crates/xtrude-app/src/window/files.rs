// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The document's file lifecycle: New, Open (and Open Recent), Save, Save As,
//! the imports and exports, the document's own camera.

use std::path::{Path, PathBuf};

use glam::DVec3;
use serde_json::{json, Value};
use xtrude_core::camera::OrbitCamera;

use xtrude_io::step::{self, HelperBackend, StepError, Tessellation};
use xtrude_io::{dae, document, dxf_in, glb, gltf, igz, obj, skp, stl, threemf};
use xtrude_model::history::{CmdError, Command, FnCommand};
use xtrude_model::i18n::tr;
use xtrude_model::saved_views::CameraHome;
use xtrude_model::scene::Scene;
use xtrude_model::settings::{self, keys};
use xtrude_model::text3d::{make_text_group, text_is_pristine, EditText3DCommand, TextParams};
use xtrude_model::units;

use super::context::CtxItem;
pub use super::progress::SkpParse;
use super::progress::{ImportJob, ImportProgress, ImportResult};
use super::{filedialogs, Asker, MainWindow, SaveChoice};
use crate::tools::place_group::PlaceGroupTool;

/// Save and Save As: the Xtrude CAD document.
pub const DOCUMENT_FILE_FILTER: &str = "Xtrude CAD document (*.xtrude);;All files (*)";

/// A file format the Open, Import and Export dialogs offer: `key` names
/// it in the code, `name` and `exts` make its line in the dialog's list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileFormat {
    pub key: &'static str,
    pub name: &'static str,
    pub exts: &'static [&'static str],
}

impl FileFormat {
    const fn new(key: &'static str, name: &'static str, exts: &'static [&'static str]) -> Self {
        FileFormat { key, name, exts }
    }
}

/// What Open and Import read, in the order their dialogs list them: the
/// documents first, then the other formats.
pub const IMPORT_FORMATS: &[FileFormat] = &[
    FileFormat::new("xtrude", "Xtrude CAD document", &["xtrude"]),
    FileFormat::new("igz", "IngeTrazo document", &["igz"]),
    FileFormat::new("step", "STEP", &["step", "stp"]),
    FileFormat::new("stl", "STL mesh", &["stl"]),
    FileFormat::new("obj", "Wavefront OBJ", &["obj"]),
    FileFormat::new("glb", "glTF binary", &["glb", "gltf"]),
    FileFormat::new("dae", "COLLADA", &["dae"]),
    FileFormat::new("dxf", "AutoCAD DXF", &["dxf"]),
    FileFormat::new("skp", "SKP", &["skp"]),
];

/// What Export writes, in the order its dialog lists them.
pub const EXPORT_FORMATS: &[FileFormat] = &[
    FileFormat::new("stl", "STL mesh, millimetres", &["stl"]),
    FileFormat::new("3mf", "3MF package, millimetres", &["3mf"]),
    FileFormat::new("obj", "Wavefront OBJ", &["obj"]),
    FileFormat::new("glb", "glTF binary", &["glb"]),
    FileFormat::new("dae", "COLLADA", &["dae"]),
];

/// A file dialog's filter for `formats`: "Supported formats" (every
/// extension) first when `all_first`, then one line per format, then
/// "All files".
pub fn formats_filter(formats: &[FileFormat], all_first: bool) -> String {
    let patterns = |exts: &[&str]| {
        exts.iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut lines = Vec::new();
    if all_first {
        let every: Vec<&str> = formats
            .iter()
            .flat_map(|f| f.exts.iter().copied())
            .collect();
        lines.push(format!(
            "{} ({})",
            tr("Supported formats", &[]),
            patterns(&every)
        ));
    }
    for f in formats {
        lines.push(format!("{} ({})", tr(f.name, &[]), patterns(f.exts)));
    }
    lines.push(tr("All files (*)", &[]));
    lines.join(";;")
}

/// The format among `formats` that `path`'s extension names.
pub fn format_of(formats: &[FileFormat], path: &Path) -> Option<FileFormat> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    formats
        .iter()
        .find(|f| f.exts.contains(&ext.as_str()))
        .copied()
}

/// The working volume an empty document opens on, in metres, by its
/// length unit: a printer bed's worth for millimetres and centimetres, ten
/// inches for inches. Metres and feet keep the default view.
fn new_document_volume(unit: &str) -> Option<f64> {
    match unit {
        "mm" | "cm" => Some(0.25),
        "in" | "in-frac" => Some(0.254),
        _ => None,
    }
}

/// Read the document at `path` into `scene`: an IngeTrazo `.igz` by its
/// reader, anything else as an Xtrude CAD document.
fn load_document(scene: &mut Scene, path: &Path) -> Result<(), String> {
    if igz::is_igz_path(path) {
        igz::load_into(scene, path, None).map_err(|e| e.to_string())
    } else {
        document::load_into(scene, path, None).map_err(|e| e.to_string())
    }
}

const RECENT_MAX: usize = 10;

/// The last documents, most recent first.
pub fn recent_paths() -> Vec<String> {
    match settings::read(&keys::RECENT_FILES) {
        Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
            .collect(),
        Value::String(s) if !s.trim().is_empty() => vec![s],
        _ => Vec::new(),
    }
}

/// Put `path` at the top of File ▸ Open Recent (Xtrude CAD documents
/// only).
pub fn remember_recent(path: &Path) {
    if !document::is_document_path(path) {
        return;
    }
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let mut recent: Vec<String> = recent_paths().into_iter().filter(|x| *x != key).collect();
    recent.insert(0, key);
    recent.truncate(RECENT_MAX);
    settings::write(&keys::RECENT_FILES, &json!(recent));
}

pub fn clear_recent() {
    settings::write(&keys::RECENT_FILES, &json!([]));
}

/// File ▸ Open Recent: the documents still there, numbered, then Clear
/// list.
pub fn recent_menu() -> Vec<CtxItem> {
    let alive: Vec<PathBuf> = recent_paths()
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();
    if alive.is_empty() {
        return vec![CtxItem::Entry {
            id: "recent_none".into(),
            text: tr("(no recent documents)", &[]),
            enabled: false,
            checked: None,
        }];
    }
    let mut out: Vec<CtxItem> = alive
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let parent = p
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            CtxItem::Entry {
                id: format!("open_recent:{}", p.display()),
                text: format!("&{}  {name}  —  {parent}", i + 1),
                enabled: true,
                checked: None,
            }
        })
        .collect();
    out.push(CtxItem::Separator);
    out.push(CtxItem::Entry {
        id: "clear_recent".into(),
        text: tr("Clear list", &[]),
        enabled: true,
        checked: None,
    });
    out
}

fn unit_labels() -> (Vec<&'static str>, Vec<String>) {
    let keys = vec!["m", "cm", "mm", "in", "ft"];
    let labels = ["Metres", "Centimetres", "Millimetres", "Inches", "Feet"]
        .iter()
        .map(|l| tr(l, &[]))
        .collect();
    (keys, labels)
}

impl MainWindow {
    // ---- new document --------------------------------------------------------------------
    /// A new, empty document starts in the units chosen for new documents
    /// — not a change to the document.
    pub(super) fn apply_new_document_units(&mut self) {
        let u = units::new_document_units();
        self.viewport.scene.apply_units(&u);
    }

    /// Frame an empty document on a working volume sized for its units.
    pub(super) fn frame_new_document(&mut self) {
        let unit = self.viewport.scene.units.length.clone();
        let Some(side) = new_document_volume(&unit) else {
            return;
        };
        self.viewport
            .camera
            .fit_to(DVec3::ZERO, DVec3::new(side, side, side * 0.5), 1.3);
        self.viewport.update();
    }

    /// True when it is safe to discard the current drawing.
    pub fn confirm_discard(&mut self, prompt: &str) -> bool {
        if !self.is_dirty() {
            return true;
        }
        let text = tr(
            "{prompt}\n\nUnsaved changes will be lost.",
            &[("prompt", &prompt)],
        );
        match self.dialogs.unsaved_changes(&text) {
            SaveChoice::Save => {
                self.on_save();
                !self.is_dirty()
            }
            SaveChoice::Discard => true,
            SaveChoice::Cancel => false,
        }
    }

    pub fn on_new(&mut self) {
        self.viewport.end_group_edit();
        if !self.confirm_discard(&tr("Discard current drawing?", &[])) {
            return;
        }
        self.clear_document();
    }

    /// Replace the document with a new, empty one, without asking.
    fn clear_document(&mut self) {
        self.viewport.scene.clear();
        self.viewport.scene.version += 1;
        self.viewport.history.clear();
        self.reset_camera_home();
        self.viewport.reset_document_caches();
        self.current_path = None;
        self.import_name = None;
        self.scene_serial += 1;
        self.apply_new_document_units();
        self.frame_new_document();
        self.viewport.notify_scene_changed();
        self.mark_saved_if_untouched();
        self.sync_style_menu();
        self.update_title();
    }

    /// A cleared document counts as saved (the version bump of the clear
    /// is not "a change" — the new document starts clean).
    fn mark_saved_if_untouched(&mut self) {
        self.mark_saved();
    }

    /// How to start another Xtrude CAD from this one: this very program.
    pub fn new_window_command() -> Option<std::path::PathBuf> {
        std::env::current_exe().ok()
    }

    pub fn on_new_window(&mut self) {
        let Some(exe) = Self::new_window_command() else {
            self.dialogs.warn(
                &tr("New Window", &[]),
                &tr("Could not start another Xtrude CAD window.", &[]),
            );
            return;
        };
        if std::process::Command::new(exe).spawn().is_err() {
            self.dialogs.warn(
                &tr("New Window", &[]),
                &tr("Could not start another Xtrude CAD window.", &[]),
            );
        }
    }

    /// The window lost focus: the copy goes to the system clipboard now,
    /// for another window to paste.
    pub fn on_focus_lost(&mut self) {
        self.system_clipboard.export();
    }

    pub fn request_quit(&mut self) {
        self.viewport.end_group_edit();
        if self.confirm_discard(&tr("Quit Xtrude CAD?", &[])) {
            self.quit_requested = true;
            // the copy outlives this window as plain data
            super::clipboard::flush(self.system_clipboard.as_mut());
        }
    }

    // ---- the document's camera ------------------------------------------------------
    pub fn camera_home(&self) -> CameraHome {
        let cam = &self.viewport.camera;
        CameraHome {
            target: Some(cam.target.to_array()),
            distance: Some(cam.distance),
            yaw: Some(cam.yaw),
            pitch: Some(cam.pitch),
            fov_deg: Some(cam.fov_deg),
            perspective: Some(cam.perspective),
            extra: Default::default(),
        }
    }

    /// Point the camera as `home` says; a field it lacks stays as it is.
    pub fn apply_camera(&mut self, home: &CameraHome) {
        let cam = &mut self.viewport.camera;
        if let Some(t) = home.target {
            cam.target = DVec3::from_array(t);
        }
        if let Some(x) = home.distance {
            cam.distance = x;
        }
        if let Some(x) = home.yaw {
            cam.yaw = x;
        }
        if let Some(x) = home.pitch {
            cam.pitch = x;
        }
        if let Some(x) = home.fov_deg {
            cam.fov_deg = x;
        }
        if let Some(b) = home.perspective {
            cam.perspective = b;
        }
        self.viewport.update();
        self.sync_projection_action();
    }

    /// Look at what the document's author was looking at; a document from
    /// before the camera was saved is framed whole.
    fn apply_camera_home(&mut self) {
        match self.viewport.scene.camera_home.clone() {
            Some(home) => self.apply_camera(&home),
            None => self.on_zoom_extents(),
        }
    }

    fn reset_camera_home(&mut self) {
        let fresh = OrbitCamera::default();
        let cam = &mut self.viewport.camera;
        cam.target = fresh.target;
        cam.distance = fresh.distance;
        cam.yaw = fresh.yaw;
        cam.pitch = fresh.pitch;
        cam.fov_deg = fresh.fov_deg;
        cam.perspective = fresh.perspective;
        self.viewport.update();
        self.sync_projection_action();
    }

    // ---- file dialogs ----------------------------------------------------------------------
    /// The open document's folder, the dialogs' fallback start.
    pub(super) fn document_folder(&self) -> Option<PathBuf> {
        self.current_path
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|f| f.is_dir())
            .map(Path::to_path_buf)
    }

    /// A file to read, starting where the user last was; the pick is
    /// remembered.
    pub fn pick_open(&mut self, title: &str, filter: &str) -> Option<PathBuf> {
        let start = filedialogs::start_in("", self.document_folder().as_deref());
        let picked = self.dialogs.open_file(title, filter, &start);
        filedialogs::remember(picked.as_deref());
        picked
    }

    /// A file to write, `suggested` (a bare name, or a path) placed where
    /// the user last was; the pick is remembered.
    pub fn pick_save(&mut self, title: &str, filter: &str, suggested: &str) -> Option<PathBuf> {
        let start = filedialogs::start_in(suggested, self.document_folder().as_deref());
        let picked = self.dialogs.save_file(title, filter, &start);
        filedialogs::remember(picked.as_deref());
        picked
    }

    // ---- open / save -----------------------------------------------------------------------
    pub fn on_open(&mut self) {
        self.viewport.end_group_edit();
        if !self.confirm_discard(&tr("Discard current drawing and open another?", &[])) {
            return;
        }
        let Some(path) = self.pick_open(&tr("Open", &[]), &formats_filter(IMPORT_FORMATS, true))
        else {
            return;
        };
        // a format that is not a document starts a new one and comes in
        // as its import does
        match format_of(IMPORT_FORMATS, &path) {
            Some(f) if !matches!(f.key, "xtrude" | "igz") => {
                self.clear_document();
                self.import_as(f, &path);
            }
            _ => {
                self.open_path(&path);
            }
        }
    }

    pub fn open_recent(&mut self, path: &Path) {
        if !path.exists() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.dialogs.warn(
                &tr("Open Recent", &[]),
                &tr("{name} is no longer there.", &[("name", &name)]),
            );
            return;
        }
        self.viewport.end_group_edit();
        if !self.confirm_discard(&tr("Discard current drawing and open another?", &[])) {
            return;
        }
        self.open_path(path);
    }

    /// The document given on the command line: a `.skp` or a STEP file is
    /// imported after the window's first paint (a big model parses for
    /// seconds — the window must be visible, not frozen before it shows),
    /// and so is an STL or an OBJ, whose import asks a question first;
    /// anything else opens now.
    pub fn open_on_start(&mut self, path: &Path) {
        let deferred = path.extension().is_some_and(|e| {
            ["skp", "stl", "obj"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        });
        if deferred || step::is_step_path(path) {
            self.pending_import = Some(path.to_path_buf());
        } else {
            self.open_path(path);
        }
    }

    /// Run the import [`Self::open_on_start`] deferred, if any.
    pub fn open_pending(&mut self) -> bool {
        match self.pending_import.take() {
            Some(path) => self.open_as_asker(&path),
            None => false,
        }
    }

    /// [`Self::open_path`] as an [`Asker`], so that the question an STL or
    /// an OBJ import asks is answered and the import replays with the
    /// answer; a cancelled question leaves the document as it was.
    pub fn open_as_asker(&mut self, path: &Path) -> bool {
        self.ask_as(Asker::Open(path.to_path_buf()), |w| w.open_path(path))
    }

    /// Open a document at `path` (File dialog, command line, double-click).
    /// An IngeTrazo `.igz` opens as a new, unsaved document with no path,
    /// so Save asks where to write the Xtrude CAD document and the `.igz`
    /// is never overwritten. `.dae`, `.skp`, `.dxf`, `.step`, `.stp`,
    /// `.stl` and `.obj` import instead.
    pub fn open_path(&mut self, path: &Path) -> bool {
        let suffix = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match suffix.as_str() {
            "dae" => return self.import_dae_path(path),
            "skp" => return self.import_skp_path(path),
            "dxf" => return self.import_dxf_path(path),
            "step" | "stp" => return self.import_step_path(path),
            "stl" => return self.import_stl_path(path),
            "obj" => return self.import_obj_path(path),
            _ => {}
        }
        let from_igz = igz::is_igz_path(path);
        let mut fresh = Scene::new();
        if let Err(e) = load_document(&mut fresh, path) {
            self.dialogs.warn(&tr("Open failed", &[]), &e);
            return false;
        }
        self.viewport.scene = fresh;
        self.viewport.history.clear();
        self.viewport.reset_document_caches();
        self.scene_serial += 1;
        self.apply_camera_home();
        if from_igz {
            self.current_path = None;
            self.import_name = Some(file_name(path));
            self.set_saved_version(-1);
        } else {
            self.current_path = Some(path.to_path_buf());
            remember_recent(path);
            self.import_name = None;
            self.mark_saved();
        }
        self.sync_style_menu();
        let repaired = self.viewport.scene.load_repairs;
        if repaired > 0 {
            // Pieces with a coordinate that is not a number were left out:
            // keep it unsaved, so Ctrl+S writes the cleaned file.
            self.set_saved_version(-1);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.dialogs.warn(
                &tr("Document repaired", &[]),
                &tr(
                    "{n} damaged pieces (a coordinate that is not a number) were left out so the rest of “{name}” could open. Save it to keep the repaired document.",
                    &[("n", &repaired), ("name", &name)],
                ),
            );
        }
        self.viewport.notify_scene_changed();
        self.update_title();
        true
    }

    pub fn on_save(&mut self) {
        self.viewport.end_group_edit();
        match self.current_path.clone() {
            None => self.on_save_as(),
            Some(p) => self.do_save(&p),
        }
    }

    pub fn on_save_as(&mut self) {
        self.viewport.end_group_edit();
        // a document opened from an `.igz` is offered under its name
        let opened_igz = self
            .import_name
            .as_deref()
            .map(Path::new)
            .filter(|n| igz::is_igz_path(n));
        let default_name = self
            .current_path
            .as_deref()
            .or(opened_igz)
            .and_then(Path::file_stem)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        let default_name = format!("{default_name}.{}", document::EXTENSION);
        let Some(mut path) = self.pick_save(
            &tr("Save Xtrude CAD document", &[]),
            &tr(DOCUMENT_FILE_FILTER, &[]),
            &default_name,
        ) else {
            return;
        };
        if !document::is_document_path(&path) {
            path.set_extension(document::EXTENSION);
        }
        self.do_save(&path);
    }

    /// Write the document; a backup of the previous file first
    /// (Preferences ▸ General).
    pub fn do_save(&mut self, path: &Path) {
        if settings::read(&keys::BACKUP) && path.is_file() {
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            if std::fs::copy(path, PathBuf::from(bak)).is_err() {
                self.show_message(&tr("Could not write the backup copy.", &[]), 4000);
            }
        }
        self.viewport.scene.camera_home = Some(self.camera_home());
        if let Err(e) = document::save_scene(&self.viewport.scene, path) {
            self.dialogs.warn(&tr("Save failed", &[]), &e.to_string());
            return;
        }
        self.current_path = Some(path.to_path_buf());
        remember_recent(path);
        self.mark_saved();
    }

    // ---- imports ---------------------------------------------------------------------------
    /// File ▸ Import: one dialog listing every format; the file's
    /// extension says which import runs.
    pub(super) fn on_import(&mut self) {
        let Some(path) = self.pick_open(&tr("Import", &[]), &formats_filter(IMPORT_FORMATS, true))
        else {
            return;
        };
        match format_of(IMPORT_FORMATS, &path) {
            Some(f) => self.import_as(f, &path),
            None => self.dialogs.warn(
                &tr("Import", &[]),
                &tr(
                    "Xtrude CAD cannot import “{name}”: its format is not one it reads.",
                    &[("name", &file_name(&path))],
                ),
            ),
        }
    }

    /// Bring the file at `path`, in format `f`, into the document.
    fn import_as(&mut self, f: FileFormat, path: &Path) {
        match f.key {
            "xtrude" => self.on_import_document_path(path, "Import Xtrude CAD document"),
            "igz" => self.on_import_document_path(path, "Import IngeTrazo document"),
            "skp" => {
                self.import_skp_path(path);
            }
            "dae" => {
                self.import_dae_path(path);
            }
            "glb" => self.import_glb_path(path),
            "obj" => {
                self.import_obj_path(path);
            }
            "stl" => {
                self.import_stl_path(path);
            }
            "step" => {
                self.import_step_path(path);
            }
            _ => {
                self.import_dxf_path(path);
            }
        }
    }

    /// Run a loader as one undo step; afterwards frame the model and name
    /// the window after the file.
    fn run_import(
        &mut self,
        path: &Path,
        failed: &str,
        load: impl FnMut(&mut Scene) -> Result<(), String>,
    ) -> bool {
        let mut load = load;
        let mut cmd = FnCommand::new("SnapshotImport", |s: &mut Scene| {
            load(s).map_err(CmdError::msg)
        });
        if !self.viewport.execute(&mut cmd) {
            let msg = self.viewport.history.last_error.clone().unwrap_or_default();
            self.dialogs.warn(&tr(failed, &[]), &msg);
            return false;
        }
        self.on_zoom_extents();
        self.viewport.update();
        let name = file_name(path);
        self.import_name = Some(name.clone());
        self.update_title();
        self.show_message(&tr("Imported {name}", &[("name", &name)]), 3000);
        true
    }

    pub fn import_dae_path(&mut self, path: &Path) -> bool {
        let p = path.to_path_buf();
        self.run_import(path, "Import DAE failed", move |s| {
            dae::load_dae(s, &p, None).map_err(|e| e.to_string())
        })
    }

    /// Import a `.skp` with the built-in reader (`import_skp_path`). The
    /// heavy parse runs on a worker, outside the undo history, so a failed
    /// or empty parse never leaves a half-applied edit; the payload then
    /// lands as one undo step ([`Self::finish_import`]). A file the reader
    /// cannot read is reported, with the way around it.
    pub fn import_skp_path(&mut self, path: &Path) -> bool {
        if skp::can_handle(path) {
            self.import_progress(&tr("Importing {name}…", &[("name", &file_name(path))]));
            match ImportJob::skp(path) {
                Ok(job) => return self.start_import(job),
                Err(e) => {
                    self.close_progress();
                    self.dialogs
                        .warn(&tr("Import SKP failed", &[]), &e.to_string());
                    return false;
                }
            }
        }
        self.skp_unreadable(path);
        false
    }

    /// Import a STEP file (`.step`, `.stp`). The `xtrude-step` helper
    /// reads and tessellates it in its own process, with the tolerances of
    /// the `import/step_tessellation` setting, while the progress dialog
    /// offers Cancel; the result then lands as one undo step
    /// ([`Self::finish_import`]), and anything the import left out is
    /// reported.
    pub fn import_step_path(&mut self, path: &Path) -> bool {
        let helper = self
            .step_helper
            .clone()
            .map_or_else(HelperBackend::locate, HelperBackend::new);
        let tessellation = Tessellation::from_json(&settings::read(&keys::STEP_TESSELLATION).0);
        self.import_progress(&tr("Importing {name}…", &[("name", &file_name(path))]));
        match ImportJob::step(path, helper, tessellation) {
            Ok(job) => self.start_import(job),
            Err(e) => {
                self.close_progress();
                self.dialogs
                    .warn(&tr("Import STEP failed", &[]), &e.to_string());
                false
            }
        }
    }

    /// Cancel the running import, when it can stop (the dialog's Cancel).
    pub fn cancel_import(&mut self) {
        let Some(job) = &self.import_job else {
            return;
        };
        if job.can_cancel() {
            job.cancel();
            if let Some(p) = self.progress.as_mut() {
                p.label = tr("Cancelling…", &[]);
            }
        }
    }

    /// No converter behind this: the built-in reader only.
    fn skp_unreadable(&mut self, path: &Path) {
        self.dialogs.warn(
            &tr("Import SKP", &[]),
            &tr(
                "Xtrude CAD could not read {name} with its built-in .skp reader.\n\nOpen it in the program it came from and export it as COLLADA (.dae) or OBJ, then import that file here.",
                &[("name", &file_name(path))],
            ),
        );
    }

    /// A fresh progress dialog for an open or import:
    /// the previous one goes, never more than one alive.
    pub fn import_progress(&mut self, title: &str) {
        self.progress = Some(ImportProgress::new(title));
    }

    pub fn close_progress(&mut self) {
        self.progress = None;
    }

    /// Hand a parse to the window. The shell polls it each frame
    /// ([`Self::poll_import`]) while the dialog shows its progress;
    /// without [`MainWindow::background_imports`] it is waited for here.
    /// `true` = the import went ahead (or is under way).
    fn start_import(&mut self, job: ImportJob) -> bool {
        self.import_job = Some(job);
        if self.background_imports {
            return true;
        }
        self.wait_import()
    }

    /// Block until the running import is done and apply it.
    pub fn wait_import(&mut self) -> bool {
        let Some(job) = self.import_job.take() else {
            return false;
        };
        let path = job.path.clone();
        let progress = &mut self.progress;
        let result = job.wait(&mut |f, t| {
            if let Some(p) = progress.as_mut() {
                p.report(f, t);
            }
        });
        self.finish_import(&path, result)
    }

    /// The shell's per-frame look at the running import: its progress so
    /// far into the dialog; once parsed, the import applied. `None` while
    /// it runs (or with nothing running), else whether it succeeded.
    pub fn poll_import(&mut self) -> Option<bool> {
        let job = self.import_job.as_mut()?;
        let progress = &mut self.progress;
        let result = job.poll(&mut |f, t| {
            if let Some(p) = progress.as_mut() {
                p.report(f, t);
            }
        })?;
        let path = self.import_job.take()?.path;
        Some(self.finish_import(&path, result))
    }

    fn report_progress(&mut self, fraction: f64, text: &str) {
        if let Some(p) = self.progress.as_mut() {
            p.report(fraction, text);
        }
    }

    /// Apply a parsed import as one undo step, frame it and name the
    /// window after the file; failures close the dialog, then say why.
    fn finish_import(&mut self, path: &Path, result: ImportResult) -> bool {
        let name = file_name(path);
        match result {
            ImportResult::Skp(Err(SkpParse::NeedsConverter)) => {
                self.close_progress();
                self.skp_unreadable(path);
                false
            }
            ImportResult::Skp(Err(SkpParse::Failed(msg))) => {
                self.close_progress();
                self.dialogs.warn(&tr("Import SKP failed", &[]), &msg);
                false
            }
            ImportResult::Stl(Err(e)) => {
                self.close_progress();
                self.dialogs
                    .warn(&tr("Import STL failed", &[]), &e.to_string());
                false
            }
            ImportResult::Step(Err(StepError::Cancelled)) => {
                self.close_progress();
                self.show_message(&tr("Import of {name} cancelled", &[("name", &name)]), 3000);
                false
            }
            ImportResult::Step(Err(e)) => {
                self.close_progress();
                self.dialogs
                    .warn(&tr("Import STEP failed", &[]), &e.to_string());
                false
            }
            ImportResult::Step(Ok(payload)) => {
                let mut cmd = FnCommand::new("SnapshotImport", |s: &mut Scene| {
                    step::apply_payload(s, &payload).map_err(CmdError::msg)
                });
                if !self.apply_import(&mut cmd, "Import STEP failed") {
                    return false;
                }
                self.import_name = Some(name.clone());
                self.update_title();
                self.show_message(&tr("Imported {name}", &[("name", &name)]), 3000);
                if !payload.warnings.is_empty() {
                    let lines: Vec<String> =
                        payload.warnings.iter().map(|w| format!("• {w}")).collect();
                    self.dialogs.report(
                        &tr("Imported {name}", &[("name", &name)]),
                        &tr(
                            "Some of {name} did not import as it is in the file:\n\n{list}",
                            &[("name", &name), ("list", &lines.join("\n"))],
                        ),
                    );
                }
                true
            }
            ImportResult::Skp(Ok(payload)) => {
                let mut cmd = FnCommand::new("SnapshotImport", |s: &mut Scene| {
                    skp::apply_payload(s, &payload)
                        .map(|_| ())
                        .map_err(CmdError::msg)
                });
                if !self.apply_import(&mut cmd, "Import SKP failed") {
                    return false;
                }
                self.import_name = Some(name.clone());
                self.update_title();
                if payload.empty {
                    // A template or blank file: say so, or an
                    // empty viewport reads as a failed import.
                    self.show_message(
                        &tr(
                            "{name} has no geometry — nothing to import.",
                            &[("name", &name)],
                        ),
                        8000,
                    );
                } else {
                    self.show_message(&tr("Imported {name}", &[("name", &name)]), 3000);
                }
                true
            }
            ImportResult::Stl(Ok(mesh)) => {
                let p = path.to_path_buf();
                let mut target = Some(mesh);
                let mut cmd = FnCommand::new("SnapshotImport", |s: &mut Scene| {
                    if let Some(m) = target.take() {
                        stl::add_stl_mesh(s, &p, m);
                    }
                    Ok(())
                });
                if !self.apply_import(&mut cmd, "Import STL failed") {
                    return false;
                }
                self.import_name = Some(name.clone());
                self.update_title();
                self.show_message(&tr("Imported {name}", &[("name", &name)]), 3000);
                true
            }
        }
    }

    /// Execute a parsed import's command, then the last milestones
    /// and the framing, and close the dialog.
    fn apply_import(&mut self, cmd: &mut dyn Command, failed: &str) -> bool {
        if !self.viewport.execute(cmd) {
            self.close_progress();
            let msg = self.viewport.history.last_error.clone().unwrap_or_default();
            self.dialogs.warn(&tr(failed, &[]), &msg);
            return false;
        }
        self.report_progress(0.97, "Preparing display…");
        self.report_progress(1.0, "Done");
        self.on_zoom_extents();
        self.close_progress();
        self.viewport.update();
        true
    }

    pub fn import_glb_path(&mut self, path: &Path) {
        let p = path.to_path_buf();
        self.run_import(path, "Import glTF/GLB failed", move |s| {
            glb::load_glb(s, &p, None).map_err(|e| e.to_string())
        });
    }

    /// The metres-per-unit factor of an OBJ, asked once per import with the
    /// likely answer preselected; `None` = cancel the import.
    fn obj_unit(&mut self, path: &Path) -> Option<f64> {
        let (keys, labels) = unit_labels();
        let mut guess = obj::suggest_unit(path).to_string();
        if guess == "m" {
            guess = settings::read(&keys::OBJ_UNIT);
        }
        let idx = keys.iter().position(|k| *k == guess).unwrap_or(0);
        let chosen = self.dialogs.choose(
            &tr("Import OBJ", &[]),
            &tr(
                "An OBJ file does not record its unit. What is this model in?",
                &[],
            ),
            &labels,
            idx,
        )?;
        let key = keys.get(chosen)?;
        settings::write(&keys::OBJ_UNIT, &key.to_string());
        obj::OBJ_UNITS
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, f)| *f)
    }

    pub fn import_obj_path(&mut self, path: &Path) -> bool {
        let Some(scale) = self.obj_unit(path) else {
            return false;
        };
        let p = path.to_path_buf();
        self.run_import(path, "Import OBJ failed", move |s| {
            obj::load_obj(s, &p, None, scale, None).map_err(|e| e.to_string())
        })
    }

    pub fn import_stl_path(&mut self, path: &Path) -> bool {
        let (keys, labels) = unit_labels();
        let guess = settings::read(&keys::STL_UNIT);
        let idx = keys.iter().position(|k| *k == guess).unwrap_or(2);
        let modes = [
            ("none", "No mesh simplification"),
            (
                "principal",
                "Merge coplanar triangles on XY, XZ and YZ planes",
            ),
            ("all", "Advanced: merge all coplanar surfaces"),
        ];
        let mode_labels: Vec<String> = modes.iter().map(|(_, l)| tr(l, &[])).collect();
        let saved_mode = settings::read(&keys::STL_SIMPLIFY_MODE);
        let mode_idx = modes
            .iter()
            .position(|(k, _)| *k == saved_mode)
            .unwrap_or(1);
        let Some((u, m)) = self
            .dialogs
            .stl_options(&labels, idx, &mode_labels, mode_idx)
        else {
            return false;
        };
        let (Some(unit), Some((mode_key, _))) = (keys.get(u), modes.get(m)) else {
            return false;
        };
        settings::write(&keys::STL_UNIT, &unit.to_string());
        settings::write(&keys::STL_SIMPLIFY_MODE, &mode_key.to_string());
        let scale = stl::unit_scale(unit).unwrap_or(0.001);
        let mode = match *mode_key {
            "none" => stl::SimplifyMode::None,
            "all" => stl::SimplifyMode::All,
            _ => stl::SimplifyMode::Principal,
        };
        self.import_progress(&tr("Importing {name}…", &[("name", &file_name(path))]));
        match ImportJob::stl(path, scale, mode) {
            Ok(job) => self.start_import(job),
            Err(e) => {
                self.close_progress();
                self.dialogs
                    .warn(&tr("Import STL failed", &[]), &e.to_string());
                false
            }
        }
    }

    /// Confirm a DXF's unit, the measured suggestion preselected.
    fn dxf_unit(&mut self, header: Option<f64>) -> Option<f64> {
        let keys = ["mm", "cm", "m", "in", "ft"];
        let factors = [0.001, 0.01, 1.0, 0.0254, 0.3048];
        let labels: Vec<String> = ["Millimetres", "Centimetres", "Metres", "Inches", "Feet"]
            .iter()
            .map(|l| tr(l, &[]))
            .collect();
        let last = header
            .and_then(|h| factors.iter().position(|f| (f - h).abs() < 1e-12))
            .map(|i| keys[i].to_string())
            .unwrap_or_else(|| settings::read(&keys::DXF_UNIT));
        let idx = keys.iter().position(|k| *k == last).unwrap_or(0);
        let chosen = self.dialogs.choose(
            &tr("Import DXF", &[]),
            &tr("What unit is this drawing in? (CAD headers often lie)", &[]),
            &labels,
            idx,
        )?;
        settings::write(&settings::keys::DXF_UNIT, &keys.get(chosen)?.to_string());
        factors.get(chosen).copied()
    }

    pub fn import_dxf_path(&mut self, path: &Path) -> bool {
        let doc = match dxf_in::open_document(path) {
            Ok(d) => d,
            Err(e) => {
                self.dialogs
                    .warn(&tr("Import DXF failed", &[]), &e.to_string());
                return false;
            }
        };
        let (suggested, _code) = dxf_in::suggest_unit_scale(&doc);
        let Some(scale) = self.dxf_unit(suggested) else {
            return false;
        };
        let p = path.to_path_buf();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut stats = None;
        let ok = {
            let stats_ref = &mut stats;
            self.run_import(path, "Import DXF failed", |s| {
                let st = dxf_in::load_dxf(s, &p, None, scale, Some(&doc), Some(&stem))
                    .map_err(|e| e.to_string())?;
                *stats_ref = Some(st);
                Ok(())
            })
        };
        if !ok {
            return false;
        }
        if let Some(st) = stats {
            let mut msg = tr(
                "Imported {name}: {edges} edges in {groups} tagged groups",
                &[
                    ("name", &file_name(path)),
                    ("edges", &st.edges),
                    ("groups", &st.groups),
                ],
            );
            if let Some(o) = st.offset {
                msg.push_str("  ·  ");
                msg.push_str(&tr(
                    "moved near the origin (survey coordinates); offset {x}, {y} m",
                    &[("x", &format!("{:.0}", o.x)), ("y", &format!("{:.0}", o.y))],
                ));
            }
            self.show_message(&msg, 8000);
        }
        true
    }

    /// File ▸ Import of an Xtrude CAD or IngeTrazo document; `title` names
    /// the import in its messages.
    fn on_import_document_path(&mut self, path: &Path, title: &str) {
        if let Some(cur) = &self.current_path {
            let same = path.canonicalize().ok() == cur.canonicalize().ok();
            if same {
                self.dialogs.warn(
                    &tr(title, &[]),
                    &tr("That is the document you are editing.", &[]),
                );
                return;
            }
        }
        match self.import_document_path(path, None) {
            Err(e) => self
                .dialogs
                .warn(&tr("{title} failed", &[("title", &tr(title, &[]))]), &e),
            Ok(None) => self.dialogs.warn(
                &tr(title, &[]),
                &tr("“{name}” has no geometry.", &[("name", &file_name(path))]),
            ),
            Ok(Some(_)) => {}
        }
    }

    /// Insert the document at `path` (an Xtrude CAD document, or an
    /// IngeTrazo `.igz`) as ONE component: `at = None` hands it to the
    /// placement tool; a point inserts it with its origin there at once. A
    /// file that cannot be read changes nothing.
    pub fn import_document_path(
        &mut self,
        path: &Path,
        at: Option<DVec3>,
    ) -> Result<Option<xtrude_model::ids::GroupId>, String> {
        let mut temp = Scene::new();
        load_document(&mut temp, path)?;
        self.viewport.end_group_edit();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(comp) = xtrude_model::insert::import_document_as_component(
            &mut self.viewport.scene.doc,
            temp.doc,
            &stem,
        ) else {
            return Ok(None);
        };
        let id = comp.id;
        let mut tool = PlaceGroupTool::new(
            &self.viewport.scene.doc.meshes,
            comp,
            false,
            Some(DVec3::ZERO),
        );
        match at {
            None => {
                self.activate_transient(Box::new(tool));
                self.viewport
                    .flash_status(&tr("Click to place the component (Esc cancels)", &[]), 4000);
            }
            Some(p) => tool.place_at(&mut self.viewport, p),
        }
        self.pump_events();
        Ok(Some(id))
    }

    // ---- 3D text ----------------------------------------------------------------------------
    /// Ask for a 3D text and hand the tool that places it. Returns whether
    /// that tool is in hand (not when the dialog is cancelled or pending).
    pub(super) fn on_insert_3d_text(&mut self) -> bool {
        let mut seed = TextParams::new(&tr("Xtrude CAD", &[]), "Sans");
        seed.bold = true;
        let Some(params) = self.dialogs.text3d(&seed) else {
            return false;
        };
        if params.text.trim().is_empty() {
            return false;
        }
        self.viewport.end_group_edit();
        let Some(group) = make_text_group(&mut self.viewport.scene.doc.meshes, &params) else {
            self.dialogs.warn(
                &tr("3D Text", &[]),
                &tr("Could not build geometry for that text.", &[]),
            );
            return false;
        };
        let tool = PlaceGroupTool::new(&self.viewport.scene.doc.meshes, group, true, None);
        self.activate_transient(Box::new(tool));
        self.viewport
            .flash_status(&tr("Click to place the component (Esc cancels)", &[]), 4000);
        self.pump_events();
        true
    }

    /// The one selected 3D-text container (a letter picked inside the open
    /// container counts), else `None`.
    pub fn selected_text3d(&self) -> Option<xtrude_model::ids::GroupId> {
        let scene = &self.viewport.scene;
        let sel: Vec<_> = scene
            .selection
            .iter()
            .filter_map(|e| match e {
                xtrude_model::entity::EntityRef::Group(g) => Some(*g),
                _ => None,
            })
            .collect();
        if sel.len() != 1 {
            return None;
        }
        let g = scene.doc.find_group(sel[0])?;
        if g.text3d.is_some() {
            return Some(g.id);
        }
        let ctx = scene.edit_group().and_then(|c| scene.doc.find_group(c))?;
        if ctx.text3d.is_some()
            && ctx
                .children
                .iter()
                .any(|k| k.id == g.id || k.find(g.id).is_some())
        {
            return Some(ctx.id);
        }
        None
    }

    /// Reopen the 3D Text dialog on an existing text and lay the letters out
    /// again in place; a text edited by hand refuses.
    pub fn on_edit_3d_text(&mut self, group: Option<xtrude_model::ids::GroupId>) {
        let Some(gid) = group.or_else(|| self.selected_text3d()) else {
            return;
        };
        let Some(g) = self.viewport.scene.doc.find_group(gid).cloned() else {
            return;
        };
        let Some(raw) = g.text3d.clone() else {
            return;
        };
        if !text_is_pristine(&self.viewport.scene.doc.meshes, &g) {
            self.viewport.flash_status(
                &tr(
                    "This text's letters were edited by hand, so it can no longer be regenerated as text.",
                    &[],
                ),
                5000,
            );
            self.pump_events();
            return;
        }
        let current = TextParams::from_map(&raw);
        let Some(params) = self.dialogs.text3d(&current) else {
            return;
        };
        if params == current {
            return;
        }
        let mut probe = self.viewport.scene.doc.meshes.clone();
        if make_text_group(&mut probe, &params).is_none() {
            self.dialogs.warn(
                &tr("3D Text", &[]),
                &tr("Could not build geometry for that text.", &[]),
            );
            return;
        }
        if self.viewport.scene.edit_group() == Some(gid) {
            self.viewport.end_group_edit();
        }
        self.viewport.run(EditText3DCommand { group: gid, params });
        self.viewport.scene.select(
            [xtrude_model::entity::EntityRef::Group(gid)],
            xtrude_model::scene::SelectMode::Replace,
        );
        self.viewport.update();
    }

    // ---- exports ----------------------------------------------------------------------------
    /// File ▸ Export: one dialog listing every format; the file's
    /// extension says which is written. A name without one asks which.
    pub(super) fn on_export(&mut self) {
        let base = self
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        let first = EXPORT_FORMATS[0];
        let Some(mut path) = self.pick_save(
            &tr("Export", &[]),
            &formats_filter(EXPORT_FORMATS, false),
            &format!("{base}.{}", first.exts[0]),
        ) else {
            return;
        };
        let format = match format_of(EXPORT_FORMATS, &path) {
            Some(f) => f,
            None => {
                let names: Vec<String> = EXPORT_FORMATS.iter().map(|f| tr(f.name, &[])).collect();
                let Some(i) = self.dialogs.choose(
                    &tr("Export", &[]),
                    &tr("Export “{name}” as", &[("name", &file_name(&path))]),
                    &names,
                    0,
                ) else {
                    return;
                };
                let Some(&f) = EXPORT_FORMATS.get(i) else {
                    return;
                };
                let mut with_ext = path.into_os_string();
                with_ext.push(format!(".{}", f.exts[0]));
                path = PathBuf::from(with_ext);
                f
            }
        };
        let scene = &self.viewport.scene;
        let (label, result): (&str, Result<(), String>) = match format.key {
            "stl" => (
                "STL",
                stl::save_stl(scene, &path, "mm").map_err(|e| e.to_string()),
            ),
            "3mf" => (
                "3MF",
                threemf::save_3mf(scene, &path).map_err(|e| e.to_string()),
            ),
            "obj" => (
                "OBJ",
                obj::save_obj(scene, &path).map_err(|e| e.to_string()),
            ),
            "glb" => (
                "GLB",
                gltf::save_glb(scene, &path).map_err(|e| e.to_string()),
            ),
            _ => (
                "COLLADA",
                dae::save_dae(scene, &path, &base).map_err(|e| e.to_string()),
            ),
        };
        match result {
            Err(e) => self
                .dialogs
                .warn(&tr("Export {label} failed", &[("label", &label)]), &e),
            Ok(()) => self.show_message(
                &tr(
                    "Exported {label} to {name}",
                    &[("label", &label), ("name", &file_name(&path))],
                ),
                3000,
            ),
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
