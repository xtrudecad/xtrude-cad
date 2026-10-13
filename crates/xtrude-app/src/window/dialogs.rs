// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The questions the window asks (text input, message boxes, file
//! pickers), behind a trait: the egui shell answers them on screen, tests
//! script the answers.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use xtrude_ext::{ParamSpec, ParamValue};
use xtrude_model::ids::ObjId;
use xtrude_model::text3d::TextParams;

use super::MainWindow;
use crate::tray::{Tray, TrayAction};

/// The Name Section Plane form's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionName {
    pub name: String,
    pub symbol: String,
    /// «Don't ask again».
    pub dont_ask: bool,
}

/// The answer to «Unsaved changes».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

impl SaveChoice {
    /// The default button: Enter saves.
    pub const DEFAULT: SaveChoice = SaveChoice::Save;
}

/// The Unsaved Changes question's title.
pub fn unsaved_title() -> String {
    xtrude_model::i18n::tr("Unsaved changes", &[])
}

/// Its buttons, in order. «Don't Save» is worded differently per platform,
/// so all three are named here and read the same everywhere.
pub fn unsaved_buttons() -> [(String, SaveChoice); 3] {
    [
        (xtrude_model::i18n::tr("Save", &[]), SaveChoice::Save),
        (
            xtrude_model::i18n::tr("Don't Save", &[]),
            SaveChoice::Discard,
        ),
        (xtrude_model::i18n::tr("Cancel", &[]), SaveChoice::Cancel),
    ]
}

pub trait Dialogs {
    /// A line (or block) of text; `None` = cancelled.
    fn get_text(
        &mut self,
        title: &str,
        label: &str,
        current: &str,
        multiline: bool,
    ) -> Option<String>;
    /// A number in `[min, max]`; `None` = cancelled.
    fn get_number(
        &mut self,
        title: &str,
        label: &str,
        value: f64,
        min: f64,
        max: f64,
        decimals: u32,
    ) -> Option<f64>;
    /// Save / Don't Save / Cancel.
    fn unsaved_changes(&mut self, prompt: &str) -> SaveChoice;
    /// Yes or no.
    fn confirm(&mut self, title: &str, text: &str) -> bool;
    /// A message the user acknowledges.
    fn warn(&mut self, title: &str, text: &str);
    /// A longer text the user reads and closes (several lines: a list, a
    /// Lua traceback). An answerer without a dialog for it warns.
    fn report(&mut self, title: &str, text: &str) {
        self.warn(title, text)
    }
    /// A file to read (`filter` like "Xtrude CAD document (*.xtrude)"),
    /// starting in the folder `start`.
    fn open_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf>;
    /// A file to write; `start` is the suggested path (folder and name).
    fn save_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf>;
    /// One of `items`; `None` = cancelled.
    fn choose(
        &mut self,
        title: &str,
        label: &str,
        items: &[String],
        current: usize,
    ) -> Option<usize>;
    /// The STL import form: `(unit index, simplification index)`.
    fn stl_options(
        &mut self,
        units: &[String],
        unit: usize,
        modes: &[String],
        mode: usize,
    ) -> Option<(usize, usize)>;
    /// The 3D Text form, pre-filled with `params`.
    fn text3d(&mut self, params: &TextParams) -> Option<TextParams>;
    /// The Name Section Plane form.
    fn section_name(&mut self, name: &str, symbol: &str) -> Option<SectionName>;
    /// A colour picker, RGB 0–1; `None` = cancelled.
    fn get_color(&mut self, title: &str, current: [f64; 3]) -> Option<[f64; 3]>;
    /// A form with one entry per field, filled with `values`: the values
    /// given, one per field; `None` = cancelled. A length is typed as in
    /// the value box and answered in model units.
    fn ask_form(
        &mut self,
        title: &str,
        fields: &[ParamSpec],
        values: &[ParamValue],
    ) -> Option<Vec<ParamValue>>;
    /// Who asks from now on, and who asked until now. An answerer that
    /// answers later (the shell) runs the asker again with the answer;
    /// one that answers at once ignores it.
    fn set_asker(&mut self, asker: Option<Asker>) -> Option<Asker> {
        let _ = asker;
        None
    }
    /// A question is waiting for an answer that comes later (the shell's):
    /// it was answered "cancelled" for now, and the asker runs again with
    /// the answer. Code that asks a question after another checks this after
    /// the later one and stops, so it acts once, with every answer.
    fn waiting(&self) -> bool {
        false
    }
}

/// The code that asks a question, as something that can run again: an
/// answerer that answers a frame later (the shell) queues the answer and
/// runs the asker again, which finds it this time. A question asked
/// outside any asker cannot be replayed; its answer is dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum Asker {
    /// A window action: menu, toolbar, context menu or shortcut.
    Trigger(String),
    /// A side panel's control.
    Tray(TrayAction),
    /// The name of a section plane just placed with the mouse.
    SectionName(ObjId),
}

impl Asker {
    /// Run the asker again (the answer is queued by then).
    pub fn run(self, w: &mut MainWindow, tray: &mut Tray) {
        match self {
            Asker::Trigger(id) => {
                w.trigger(&id);
            }
            Asker::Tray(action) => tray.run(w, action),
            Asker::SectionName(id) => w.name_section_plane(id),
        }
        w.pump_events();
    }
}

/// No window to ask in: every question is cancelled, every warning goes to
/// the log (headless runs, tests that do not care).
#[derive(Default)]
pub struct NoDialogs;

impl Dialogs for NoDialogs {
    fn get_text(&mut self, _t: &str, _l: &str, _c: &str, _m: bool) -> Option<String> {
        None
    }
    fn get_number(
        &mut self,
        _t: &str,
        _l: &str,
        _v: f64,
        _a: f64,
        _b: f64,
        _d: u32,
    ) -> Option<f64> {
        None
    }
    fn unsaved_changes(&mut self, _prompt: &str) -> SaveChoice {
        SaveChoice::Discard
    }
    fn confirm(&mut self, _t: &str, _x: &str) -> bool {
        true
    }
    fn warn(&mut self, title: &str, text: &str) {
        eprintln!("{title}: {text}");
    }
    fn open_file(&mut self, _t: &str, _f: &str, _s: &Path) -> Option<PathBuf> {
        None
    }
    fn save_file(&mut self, _t: &str, _f: &str, _s: &Path) -> Option<PathBuf> {
        None
    }
    fn choose(&mut self, _t: &str, _l: &str, _i: &[String], _c: usize) -> Option<usize> {
        None
    }
    fn stl_options(
        &mut self,
        _u: &[String],
        _a: usize,
        _m: &[String],
        _b: usize,
    ) -> Option<(usize, usize)> {
        None
    }
    fn text3d(&mut self, _p: &TextParams) -> Option<TextParams> {
        None
    }
    fn get_color(&mut self, _t: &str, _c: [f64; 3]) -> Option<[f64; 3]> {
        None
    }
    fn section_name(&mut self, _n: &str, _s: &str) -> Option<SectionName> {
        None
    }
    fn ask_form(
        &mut self,
        _t: &str,
        _f: &[ParamSpec],
        _v: &[ParamValue],
    ) -> Option<Vec<ParamValue>> {
        None
    }
}

/// Scripted answers, for tests: each question pops the next answer of its
/// kind; with none left it is cancelled.
#[derive(Default)]
pub struct ScriptedDialogs {
    pub texts: Vec<Option<String>>,
    pub numbers: Vec<Option<f64>>,
    pub unsaved: Vec<SaveChoice>,
    pub confirms: Vec<bool>,
    pub open_paths: Vec<Option<PathBuf>>,
    pub save_paths: Vec<Option<PathBuf>>,
    pub choices: Vec<Option<usize>>,
    pub stl: Vec<Option<(usize, usize)>>,
    pub texts3d: Vec<Option<TextParams>>,
    pub section_names: Vec<Option<SectionName>>,
    pub colors: Vec<Option<[f64; 3]>>,
    pub forms: Vec<Option<Vec<ParamValue>>>,
    /// The values each form was filled with.
    pub form_values: Vec<Vec<ParamValue>>,
    /// Every question asked, `(kind, title)`, and every warning.
    pub asked: Vec<(String, String)>,
    /// Where each file dialog started.
    pub starts: Vec<PathBuf>,
    /// The filter each file dialog offered.
    pub filters: Vec<String>,
    /// The simplification each STL dialog preselected.
    pub stl_modes: Vec<usize>,
}

impl Dialogs for ScriptedDialogs {
    fn get_text(&mut self, title: &str, label: &str, current: &str, _m: bool) -> Option<String> {
        self.asked
            .push(("text".into(), format!("{title}|{label}|{current}")));
        if self.texts.is_empty() {
            None
        } else {
            self.texts.remove(0)
        }
    }
    fn get_number(
        &mut self,
        title: &str,
        _l: &str,
        _v: f64,
        _a: f64,
        _b: f64,
        _d: u32,
    ) -> Option<f64> {
        self.asked.push(("number".into(), title.into()));
        if self.numbers.is_empty() {
            None
        } else {
            self.numbers.remove(0)
        }
    }
    fn unsaved_changes(&mut self, prompt: &str) -> SaveChoice {
        self.asked.push(("unsaved".into(), prompt.into()));
        if self.unsaved.is_empty() {
            SaveChoice::Cancel
        } else {
            self.unsaved.remove(0)
        }
    }
    fn confirm(&mut self, title: &str, _x: &str) -> bool {
        self.asked.push(("confirm".into(), title.into()));
        !self.confirms.is_empty() && self.confirms.remove(0)
    }
    fn warn(&mut self, title: &str, text: &str) {
        self.asked.push(("warn".into(), format!("{title}: {text}")));
    }
    fn report(&mut self, title: &str, text: &str) {
        self.asked
            .push(("report".into(), format!("{title}: {text}")));
    }
    fn open_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.asked.push(("open".into(), title.into()));
        self.starts.push(start.to_path_buf());
        self.filters.push(filter.into());
        if self.open_paths.is_empty() {
            None
        } else {
            self.open_paths.remove(0)
        }
    }
    fn save_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.asked.push(("save".into(), title.into()));
        self.starts.push(start.to_path_buf());
        self.filters.push(filter.into());
        if self.save_paths.is_empty() {
            None
        } else {
            self.save_paths.remove(0)
        }
    }
    fn choose(&mut self, title: &str, _l: &str, _i: &[String], _c: usize) -> Option<usize> {
        self.asked.push(("choose".into(), title.into()));
        if self.choices.is_empty() {
            None
        } else {
            self.choices.remove(0)
        }
    }
    fn stl_options(
        &mut self,
        _u: &[String],
        _a: usize,
        _m: &[String],
        b: usize,
    ) -> Option<(usize, usize)> {
        self.asked.push(("stl".into(), "Import STL".into()));
        self.stl_modes.push(b);
        if self.stl.is_empty() {
            None
        } else {
            self.stl.remove(0)
        }
    }
    fn text3d(&mut self, _p: &TextParams) -> Option<TextParams> {
        self.asked.push(("text3d".into(), "3D Text".into()));
        if self.texts3d.is_empty() {
            None
        } else {
            self.texts3d.remove(0)
        }
    }
    fn get_color(&mut self, title: &str, _c: [f64; 3]) -> Option<[f64; 3]> {
        self.asked.push(("color".into(), title.into()));
        if self.colors.is_empty() {
            None
        } else {
            self.colors.remove(0)
        }
    }
    fn section_name(&mut self, name: &str, _s: &str) -> Option<SectionName> {
        self.asked.push(("section_name".into(), name.into()));
        if self.section_names.is_empty() {
            None
        } else {
            self.section_names.remove(0)
        }
    }
    fn ask_form(
        &mut self,
        title: &str,
        _f: &[ParamSpec],
        values: &[ParamValue],
    ) -> Option<Vec<ParamValue>> {
        self.asked.push(("form".into(), title.into()));
        self.form_values.push(values.to_vec());
        if self.forms.is_empty() {
            None
        } else {
            self.forms.remove(0)
        }
    }
}

/// A shared answerer: the window holds one handle, the caller keeps another
/// to read back what was asked (scripted answers in tests).
impl<D: Dialogs> Dialogs for Rc<RefCell<D>> {
    fn get_text(
        &mut self,
        title: &str,
        label: &str,
        current: &str,
        multiline: bool,
    ) -> Option<String> {
        self.borrow_mut().get_text(title, label, current, multiline)
    }
    fn get_number(
        &mut self,
        title: &str,
        label: &str,
        value: f64,
        min: f64,
        max: f64,
        decimals: u32,
    ) -> Option<f64> {
        self.borrow_mut()
            .get_number(title, label, value, min, max, decimals)
    }
    fn unsaved_changes(&mut self, prompt: &str) -> SaveChoice {
        self.borrow_mut().unsaved_changes(prompt)
    }
    fn confirm(&mut self, title: &str, text: &str) -> bool {
        self.borrow_mut().confirm(title, text)
    }
    fn warn(&mut self, title: &str, text: &str) {
        self.borrow_mut().warn(title, text)
    }
    fn report(&mut self, title: &str, text: &str) {
        self.borrow_mut().report(title, text)
    }
    fn open_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.borrow_mut().open_file(title, filter, start)
    }
    fn save_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.borrow_mut().save_file(title, filter, start)
    }
    fn choose(
        &mut self,
        title: &str,
        label: &str,
        items: &[String],
        current: usize,
    ) -> Option<usize> {
        self.borrow_mut().choose(title, label, items, current)
    }
    fn stl_options(
        &mut self,
        units: &[String],
        unit: usize,
        modes: &[String],
        mode: usize,
    ) -> Option<(usize, usize)> {
        self.borrow_mut().stl_options(units, unit, modes, mode)
    }
    fn text3d(&mut self, params: &TextParams) -> Option<TextParams> {
        self.borrow_mut().text3d(params)
    }
    fn section_name(&mut self, name: &str, symbol: &str) -> Option<SectionName> {
        self.borrow_mut().section_name(name, symbol)
    }
    fn get_color(&mut self, title: &str, current: [f64; 3]) -> Option<[f64; 3]> {
        self.borrow_mut().get_color(title, current)
    }
    fn ask_form(
        &mut self,
        title: &str,
        fields: &[ParamSpec],
        values: &[ParamValue],
    ) -> Option<Vec<ParamValue>> {
        self.borrow_mut().ask_form(title, fields, values)
    }
    fn set_asker(&mut self, asker: Option<Asker>) -> Option<Asker> {
        self.borrow_mut().set_asker(asker)
    }
    fn waiting(&self) -> bool {
        self.borrow().waiting()
    }
}
