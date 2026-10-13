// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The window's questions on screen.
//!
//! The window asks synchronously, an immediate-mode UI answers a frame
//! later: a question with no answer queued is recorded and cancelled, the
//! shell shows it, and on OK the answer is queued and the code that asked
//! (its [`Asker`]) runs again — finding it this time. Files go through the
//! desktop's own dialogs, which do block.
//!
//! An asker may ask several questions in a row (a chain): each replay
//! reads the answers from the first, so the next question comes up, and
//! the answers are kept until a replay runs to the end with no question
//! left pending. The asker stops at the question still pending
//! ([`Dialogs::waiting`]), so it acts once, with every answer. A file
//! picked in the chain is kept with the answers, so a replay does not
//! show the file dialog again.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use xtrude_ext::{ParamSpec, ParamValue};
use xtrude_model::text3d::TextParams;

use crate::tray::Tray;
use crate::window::dialogs::{Asker, Dialogs, SaveChoice, SectionName};
use crate::window::MainWindow;

/// A question waiting for the user.
#[derive(Clone, Debug)]
pub enum Question {
    Text {
        title: String,
        label: String,
        current: String,
        multiline: bool,
    },
    Number {
        title: String,
        label: String,
        value: f64,
        min: f64,
        max: f64,
        decimals: u32,
    },
    Unsaved {
        prompt: String,
    },
    Confirm {
        title: String,
        text: String,
    },
    Choose {
        title: String,
        label: String,
        items: Vec<String>,
        current: usize,
    },
    Stl {
        units: Vec<String>,
        unit: usize,
        modes: Vec<String>,
        mode: usize,
    },
    Text3d(TextParams),
    SectionName {
        name: String,
        symbol: String,
    },
    Color {
        title: String,
        current: [f64; 3],
    },
    Form {
        title: String,
        fields: Vec<ParamSpec>,
        values: Vec<ParamValue>,
    },
}

impl Question {
    /// The answer Cancel gives.
    pub fn cancelled(&self) -> Answer {
        match self {
            Question::Text { .. } => Answer::Text(None),
            Question::Number { .. } => Answer::Number(None),
            Question::Unsaved { .. } => Answer::Save(SaveChoice::Cancel),
            Question::Confirm { .. } => Answer::Bool(false),
            Question::Choose { .. } => Answer::Index(None),
            Question::Stl { .. } => Answer::Stl(None),
            Question::Text3d(_) => Answer::Text3d(None),
            Question::SectionName { .. } => Answer::Section(None),
            Question::Color { .. } => Answer::Color(None),
            Question::Form { .. } => Answer::Form(None),
        }
    }
}

/// The user's answer to a [`Question`].
#[derive(Clone, Debug)]
pub enum Answer {
    Text(Option<String>),
    Number(Option<f64>),
    Save(SaveChoice),
    Bool(bool),
    Index(Option<usize>),
    Stl(Option<(usize, usize)>),
    Text3d(Option<TextParams>),
    Section(Option<SectionName>),
    Color(Option<[f64; 3]>),
    Form(Option<Vec<ParamValue>>),
    /// A file picked to read, kept for the chain's replays.
    Opened(Option<PathBuf>),
    /// A file picked to write, kept for the chain's replays.
    Saved(Option<PathBuf>),
}

/// A question for the desktop's file dialog, which blocks and answers at
/// once.
#[derive(Clone, Debug, PartialEq)]
pub struct FilePick {
    /// A file to write; otherwise one to read.
    pub save: bool,
    pub title: String,
    /// Like "Xtrude CAD document (*.xtrude)".
    pub filter: String,
    /// The folder to start in; to save, the suggested path.
    pub start: PathBuf,
}

/// Show the desktop's file dialog.
pub fn desktop_file_dialog(pick: &FilePick) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(&pick.title);
    if pick.save {
        if let Some(dir) = pick.start.parent().filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        if let Some(name) = pick.start.file_name() {
            d = d.set_file_name(name.to_string_lossy());
        }
    } else {
        d = d.set_directory(&pick.start);
    }
    for (name, exts) in parse_filters(&pick.filter) {
        d = d.add_filter(name, &exts);
    }
    if pick.save {
        d.save_file()
    } else {
        d.pick_file()
    }
}

/// What shows the file dialogs.
pub type FilePicker = Box<dyn FnMut(&FilePick) -> Option<PathBuf>>;

/// The shell's answerer.
pub struct ShellDialogs {
    /// The answers of the chain in progress, in the order given.
    pub answers: VecDeque<Answer>,
    /// The answers this run of the asker has read (indices into `answers`).
    taken: Vec<usize>,
    pub pending: Option<Question>,
    /// Who asked the pending question; `None` when it cannot be replayed.
    pub asked_by: Option<Asker>,
    /// Who is running now (`Dialogs::set_asker`).
    asker: Option<Asker>,
    pub warnings: Vec<(String, String)>,
    /// The reports to show, one modal at a time: `(title, text)`.
    pub reports: VecDeque<(String, String)>,
    /// Shows the file dialogs: the desktop's ([`desktop_file_dialog`]), a
    /// fake in tests.
    pub files: FilePicker,
}

impl Default for ShellDialogs {
    fn default() -> Self {
        ShellDialogs {
            answers: VecDeque::new(),
            taken: Vec::new(),
            pending: None,
            asked_by: None,
            asker: None,
            warnings: Vec::new(),
            reports: VecDeque::new(),
            files: Box::new(desktop_file_dialog),
        }
    }
}

impl ShellDialogs {
    fn ask(&mut self, q: Question) {
        if self.pending.is_none() {
            self.pending = Some(q);
            self.asked_by = self.asker.clone();
        }
    }

    /// Close the pending question unanswered, and drop its chain.
    pub fn cancel(&mut self) {
        self.pending = None;
        self.asked_by = None;
        self.end_chain();
    }

    fn end_chain(&mut self) {
        self.answers.clear();
        self.taken.clear();
    }

    /// A file dialog's pick. In a chain it is an answer like the others:
    /// a replay reads it instead of showing the dialog again. No dialog
    /// shows over a question still pending.
    fn picked(&mut self, pick: FilePick) -> Option<PathBuf> {
        let fits = |a: &Answer| match a {
            Answer::Opened(_) => !pick.save,
            Answer::Saved(_) => pick.save,
            _ => false,
        };
        if let Some(i) = self.unread(fits) {
            self.taken.push(i);
            if let Some(Answer::Opened(p) | Answer::Saved(p)) = self.answers.get(i) {
                return p.clone();
            }
        }
        if self.pending.is_some() {
            return None;
        }
        let path = (self.files)(&pick);
        if self.asker.is_some() {
            self.taken.push(self.answers.len());
            self.answers.push_back(if pick.save {
                Answer::Saved(path.clone())
            } else {
                Answer::Opened(path.clone())
            });
        }
        path
    }

    /// The first answer this run has not read yet that `fits`.
    fn unread(&self, fits: impl Fn(&Answer) -> bool) -> Option<usize> {
        (0..self.answers.len()).find(|i| !self.taken.contains(i) && fits(&self.answers[*i]))
    }
}

/// Answer the pending question: the code that asked runs again and finds
/// `answer`, with the answers given before it in the chain; nothing else
/// runs. A question no asker can replay is closed and its answer dropped.
pub fn answer(
    dialogs: &RefCell<ShellDialogs>,
    answer: Answer,
    w: &mut MainWindow,
    tray: &mut Tray,
) {
    let asker = {
        let mut d = dialogs.borrow_mut();
        let asker = d.asked_by.take();
        d.pending = None;
        if asker.is_some() {
            d.answers.push_back(answer);
            d.taken.clear();
        } else {
            d.end_chain();
        }
        asker
    };
    if let Some(asker) = asker {
        asker.run(w, tray);
    }
    let mut d = dialogs.borrow_mut();
    if d.pending.is_none() {
        d.end_chain();
    }
}

/// Cancel the pending question. A question asked after others in the same
/// chain is answered "cancelled", so the code that asked goes on with the
/// answers before it, as an answerer that answers at once would; a first
/// question just closes.
pub fn dismiss(dialogs: &RefCell<ShellDialogs>, w: &mut MainWindow, tray: &mut Tray) {
    let cancelled = {
        let d = dialogs.borrow();
        d.pending
            .as_ref()
            .filter(|_| d.asked_by.is_some() && !d.answers.is_empty())
            .map(Question::cancelled)
    };
    match cancelled {
        Some(a) => answer(dialogs, a, w, tray),
        None => dialogs.borrow_mut().cancel(),
    }
}

macro_rules! take_answer {
    ($self:ident, $variant:ident, $q:expr, $default:expr) => {{
        if let Some(i) = $self.unread(|a| matches!(a, Answer::$variant(_))) {
            $self.taken.push(i);
            if let Some(Answer::$variant(a)) = $self.answers.get(i) {
                return a.clone();
            }
        }
        $self.ask($q);
        $default
    }};
}

impl Dialogs for ShellDialogs {
    fn get_text(
        &mut self,
        title: &str,
        label: &str,
        current: &str,
        multiline: bool,
    ) -> Option<String> {
        take_answer!(
            self,
            Text,
            Question::Text {
                title: title.into(),
                label: label.into(),
                current: current.into(),
                multiline,
            },
            None
        )
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
        take_answer!(
            self,
            Number,
            Question::Number {
                title: title.into(),
                label: label.into(),
                value,
                min,
                max,
                decimals,
            },
            None
        )
    }

    fn unsaved_changes(&mut self, prompt: &str) -> SaveChoice {
        take_answer!(
            self,
            Save,
            Question::Unsaved {
                prompt: prompt.into()
            },
            SaveChoice::Cancel
        )
    }

    fn confirm(&mut self, title: &str, text: &str) -> bool {
        take_answer!(
            self,
            Bool,
            Question::Confirm {
                title: title.into(),
                text: text.into(),
            },
            false
        )
    }

    fn warn(&mut self, title: &str, text: &str) {
        self.warnings.push((title.into(), text.into()));
    }

    fn report(&mut self, title: &str, text: &str) {
        self.reports.push_back((title.into(), text.into()));
    }

    fn open_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.picked(FilePick {
            save: false,
            title: title.into(),
            filter: filter.into(),
            start: start.into(),
        })
    }

    fn save_file(&mut self, title: &str, filter: &str, start: &Path) -> Option<PathBuf> {
        self.picked(FilePick {
            save: true,
            title: title.into(),
            filter: filter.into(),
            start: start.into(),
        })
    }

    fn choose(
        &mut self,
        title: &str,
        label: &str,
        items: &[String],
        current: usize,
    ) -> Option<usize> {
        take_answer!(
            self,
            Index,
            Question::Choose {
                title: title.into(),
                label: label.into(),
                items: items.to_vec(),
                current,
            },
            None
        )
    }

    fn stl_options(
        &mut self,
        units: &[String],
        unit: usize,
        modes: &[String],
        mode: usize,
    ) -> Option<(usize, usize)> {
        take_answer!(
            self,
            Stl,
            Question::Stl {
                units: units.to_vec(),
                unit,
                modes: modes.to_vec(),
                mode,
            },
            None
        )
    }

    fn text3d(&mut self, params: &TextParams) -> Option<TextParams> {
        take_answer!(self, Text3d, Question::Text3d(params.clone()), None)
    }

    fn ask_form(
        &mut self,
        title: &str,
        fields: &[ParamSpec],
        values: &[ParamValue],
    ) -> Option<Vec<ParamValue>> {
        take_answer!(
            self,
            Form,
            Question::Form {
                title: title.into(),
                fields: fields.to_vec(),
                values: values.to_vec(),
            },
            None
        )
    }

    fn get_color(&mut self, title: &str, current: [f64; 3]) -> Option<[f64; 3]> {
        take_answer!(
            self,
            Color,
            Question::Color {
                title: title.into(),
                current,
            },
            None
        )
    }

    fn section_name(&mut self, name: &str, symbol: &str) -> Option<SectionName> {
        take_answer!(
            self,
            Section,
            Question::SectionName {
                name: name.into(),
                symbol: symbol.into(),
            },
            None
        )
    }

    fn set_asker(&mut self, asker: Option<Asker>) -> Option<Asker> {
        let outer = std::mem::replace(&mut self.asker, asker);
        if self.asker.is_none() && self.pending.is_none() {
            // the asker ran to the end with every answer: the chain is done
            self.end_chain();
        }
        outer
    }

    fn waiting(&self) -> bool {
        self.pending.is_some()
    }
}

/// `"Xtrude CAD document (*.xtrude)"` → `("Xtrude CAD document", ["xtrude"])`;
/// `None` for a line with no extension ("All files (*)").
pub fn parse_filter(line: &str) -> Option<(String, Vec<String>)> {
    let open = line.find('(')?;
    let close = line[open..].find(')')? + open;
    let name = line[..open].trim().to_string();
    let exts: Vec<String> = line[open + 1..close]
        .split_whitespace()
        .filter_map(|p| p.strip_prefix("*.").map(String::from))
        .collect();
    (!exts.is_empty()).then_some((name, exts))
}

/// Every line of a filter (`;;` between lines) that names extensions, in
/// order: the dialog's list of formats, the first one selected.
pub fn parse_filters(filter: &str) -> Vec<(String, Vec<String>)> {
    filter.split(";;").filter_map(parse_filter).collect()
}
