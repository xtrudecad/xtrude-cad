// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Undo / redo history using the command pattern.
//!
//! Every mutation that should be reversible goes through a [`Command`].
//! [`History::execute`] is TRANSACTIONAL: if a command fails (an error or
//! a panic) the document is restored to its pre-command state, the failure
//! is logged, and nothing lands on the undo stack — a failed operation is a
//! no-op, never a half-committed mess.
//!
//! Design note: commands carry no hand-written inverses. Each step stores
//! the document before and after the command; meshes are copy-on-write
//! `Arc`s, so a step costs a copy of the meshes the command touched.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use crate::saved_views::SavedView;
use crate::scene::{Doc, Scene};

#[derive(Debug, Clone, thiserror::Error)]
pub enum CmdError {
    #[error("{0}")]
    Failed(String),
    #[error(transparent)]
    Mesh(#[from] xtrude_core::mesh::MeshError),
}

impl CmdError {
    pub fn msg(s: impl Into<String>) -> CmdError {
        CmdError::Failed(s.into())
    }
}

pub type CmdResult = Result<(), CmdError>;

/// A reversible operation against a [`Scene`].
/// The layer list after moving a step from `from` to `to`, merged by name
/// into the live list `cur`: the layers the step removed go, the ones it
/// added come back at their place, the ones it changed take the step's
/// record (visibility and lock stay as the user left them), and a layer
/// created outside the history (the Layers panel's +) stays.
fn merge_layers(
    from: &[crate::layers::Layer],
    to: &[crate::layers::Layer],
    cur: &[crate::layers::Layer],
) -> Vec<crate::layers::Layer> {
    let has = |list: &[crate::layers::Layer], n: &str| list.iter().any(|l| l.name == n);
    let mut out: Vec<crate::layers::Layer> = cur
        .iter()
        .filter(|l| !(has(from, &l.name) && !has(to, &l.name)))
        .cloned()
        .collect();
    for (i, ly) in to.iter().enumerate() {
        let before = from.iter().find(|l| l.name == ly.name);
        match out.iter().position(|l| l.name == ly.name) {
            Some(k) => {
                if before != Some(ly) {
                    let (visible, locked) = (out[k].visible, out[k].locked);
                    out[k] = ly.clone();
                    out[k].visible = visible;
                    out[k].locked = locked;
                }
            }
            None if before.is_none() => out.insert(i.min(out.len()), ly.clone()),
            None => {}
        }
    }
    out
}

pub trait Command {
    /// The command's name, as logged (`AddFaceCommand`, …).
    fn name(&self) -> &str;
    /// Apply the operation. Undo is the history's job.
    fn apply(&mut self, scene: &mut Scene) -> CmdResult;
    /// What this command HID (Edit ▸ Unhide ▸ Last walks the undo steps
    /// for the latest Hide).
    fn hides(&self) -> Vec<crate::entity::EntityRef> {
        Vec::new()
    }
}

struct Step {
    name: String,
    /// The entities the step hid (see [`Command::hides`]).
    hides: Vec<crate::entity::EntityRef>,
    before: Doc,
    after: Doc,
    /// Saved views the step added. They live on the `Scene`, outside the
    /// `Doc` snapshots, so undoing an import removes the views it added
    /// and redoing it puts them back.
    added_views: Vec<SavedView>,
}

pub struct History {
    undo_stack: Vec<Step>,
    redo_stack: Vec<Step>,
    /// How many steps back the history keeps (0 = unlimited).
    pub max_steps: usize,
    /// Message describing the last rolled-back failure.
    pub last_error: Option<String>,
    /// Where failed-command reports are appended (`None` = the user log dir).
    pub error_log: Option<PathBuf>,
    /// How many commands ran ([`Self::commands_run`]).
    commands_run: u64,
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

impl History {
    pub fn new() -> History {
        History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_steps: 200,
            last_error: None,
            error_log: None,
            commands_run: 0,
        }
    }

    pub fn default_error_log() -> Option<PathBuf> {
        crate::paths::user_log_dir().map(|d| d.join("xtrude-errors.log"))
    }

    /// Run `cmd` as one undo step. Returns whether it succeeded.
    pub fn execute(&mut self, scene: &mut Scene, cmd: &mut dyn Command) -> bool {
        let before = scene.doc.clone();
        let views_before: Vec<u64> = scene.saved_views.iter().map(|v| v.id).collect();
        let result = catch_unwind(AssertUnwindSafe(|| cmd.apply(scene)));
        let err = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(panic) => Some(
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into()),
            ),
        };
        if let Some(e) = err {
            scene.doc = before;
            scene.saved_views.retain(|v| views_before.contains(&v.id));
            scene.selection.clear();
            scene.version += 1;
            let msg = format!("{}: {}", cmd.name(), e);
            self.log_failure(&msg);
            self.last_error = Some(msg);
            return false;
        }
        self.last_error = None;
        scene.prune_selection();
        scene.rebind_dimensions();
        let added_views = scene
            .saved_views
            .iter()
            .filter(|v| !views_before.contains(&v.id))
            .cloned()
            .collect();
        self.undo_stack.push(Step {
            name: cmd.name().to_string(),
            hides: cmd.hides(),
            before,
            after: scene.doc.clone(),
            added_views,
        });
        if self.max_steps > 0 && self.undo_stack.len() > self.max_steps {
            let extra = self.undo_stack.len() - self.max_steps;
            self.undo_stack.drain(..extra);
        }
        self.redo_stack.clear();
        self.commands_run += 1;
        true
    }

    /// Execute an owned command (convenience for one-shot calls).
    pub fn run<C: Command>(&mut self, scene: &mut Scene, mut cmd: C) -> bool {
        self.execute(scene, &mut cmd)
    }

    fn log_failure(&self, msg: &str) {
        let text = format!(
            "[{}] command failed and was rolled back:\n{msg}\n\n",
            now_iso()
        );
        eprintln!("{text}");
        let path = self.error_log.clone().or_else(Self::default_error_log);
        if let Some(path) = path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = f.write_all(text.as_bytes());
            }
        }
    }

    /// Move the document from the step's `from` state to its `to` state,
    /// touching only what the step changed: everything else — geometry of
    /// other meshes, objects added outside the history (an import fixture, a
    /// layer toggle) — stays as it is now.
    fn restore(scene: &mut Scene, from: &Doc, to: &Doc) {
        let cur = &mut scene.doc;
        let mut ids: Vec<crate::ids::MeshId> = from
            .meshes
            .keys()
            .chain(to.meshes.keys())
            .copied()
            .collect();
        ids.sort();
        ids.dedup();
        for id in ids {
            let same = match (from.meshes.get(&id), to.meshes.get(&id)) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            match to.meshes.get(&id) {
                Some(m) => {
                    cur.meshes.insert(id, m.clone());
                }
                None => {
                    cur.meshes.remove(&id);
                }
            }
        }
        if from.groups != to.groups {
            cur.groups = to.groups.clone();
        }
        if from.dimensions != to.dimensions {
            cur.dimensions = to.dimensions.clone();
        }
        if from.text_labels != to.text_labels {
            cur.text_labels = to.text_labels.clone();
        }
        if from.guides != to.guides {
            cur.guides = to.guides.clone();
        }
        if from.layers != to.layers {
            cur.layers = merge_layers(&from.layers, &to.layers, &cur.layers);
        }
        if from.materials != to.materials {
            cur.materials = to.materials.clone();
        }
        if from.section_planes != to.section_planes {
            cur.section_planes = to.section_planes.clone();
        }
        if from.plugin_data != to.plugin_data {
            cur.plugin_data = to.plugin_data.clone();
        }
        if from.print_material != to.print_material {
            cur.print_material = to.print_material.clone();
        }
        // An open group-edit context whose group no longer resolves cannot
        // stay open.
        let valid = scene
            .edit_stack
            .iter()
            .all(|l| scene.doc.find_group(l.group).is_some());
        if !valid {
            scene.edit_stack.clear();
        }
        scene.prune_selection();
        scene.rebind_dimensions();
        scene.version += 1;
    }

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        let Some(step) = self.undo_stack.pop() else {
            return false;
        };
        Self::restore(scene, &step.after, &step.before);
        scene
            .saved_views
            .retain(|v| !step.added_views.iter().any(|a| a.id == v.id));
        self.redo_stack.push(step);
        true
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        let Some(step) = self.redo_stack.pop() else {
            return false;
        };
        Self::restore(scene, &step.before, &step.after);
        for v in &step.added_views {
            if !scene.saved_views.iter().any(|s| s.id == v.id) {
                scene.saved_views.push(v.clone());
            }
        }
        self.undo_stack.push(step);
        true
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Undo the last step and forget it (a step that turned out to be a
    /// no-op: undo, then clear the redo stack).
    pub fn discard_last(&mut self, scene: &mut Scene) -> bool {
        let ok = self.undo(scene);
        self.redo_stack.clear();
        ok
    }

    /// The entities each undo step hid, most recent first (steps that hid
    /// nothing are skipped).
    pub fn hide_steps(&self) -> impl Iterator<Item = &[crate::entity::EntityRef]> {
        self.undo_stack
            .iter()
            .rev()
            .filter(|s| !s.hides.is_empty())
            .map(|s| s.hides.as_slice())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// How many commands [`Self::execute`] ran since the history was made,
    /// failed ones not counted. It keeps counting when the undo stack is at
    /// [`Self::max_steps`], and undo and redo leave it alone.
    pub fn commands_run(&self) -> u64 {
        self.commands_run
    }

    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// The name of the step Undo would revert.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo_stack.last().map(|s| s.name.as_str())
    }

    /// Fold the last `n` steps into one (a group-edit session absorbed by
    /// its share-back). Keeps the earliest `before` and the latest `after`.
    pub fn squash_last(&mut self, n: usize, name: &str) {
        self.squash_last_with(n, name, |_| {});
    }

    /// [`Self::squash_last`], letting `fix` patch the folded step's
    /// `before` state — an instance-edit session started from a world copy
    /// of the shared definition; undoing it must land back ON the
    /// definition.
    pub fn squash_last_with(&mut self, n: usize, name: &str, fix: impl FnOnce(&mut Doc)) {
        if n < 2 || self.undo_stack.len() < n {
            if let (1, Some(last)) = (n, self.undo_stack.last_mut()) {
                last.name = name.to_string();
                fix(&mut last.before);
            }
            return;
        }
        let tail: Vec<Step> = self.undo_stack.drain(self.undo_stack.len() - n..).collect();
        let before = tail.first().map(|s| s.before.clone());
        let after = tail.last().map(|s| s.after.clone());
        if let (Some(mut before), Some(after)) = (before, after) {
            fix(&mut before);
            let hides = tail.iter().flat_map(|s| s.hides.iter().copied()).collect();
            let added_views = tail.iter().flat_map(|s| s.added_views.clone()).collect();
            self.undo_stack.push(Step {
                name: name.to_string(),
                hides,
                before,
                after,
                added_views,
            });
        }
    }
}

pub(crate) fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A command built from a closure: wrap an arbitrary mutation with
/// snapshot undo.
pub struct FnCommand<F: FnMut(&mut Scene) -> CmdResult> {
    pub name: String,
    pub f: F,
}

impl<F: FnMut(&mut Scene) -> CmdResult> FnCommand<F> {
    pub fn new(name: &str, f: F) -> Self {
        FnCommand {
            name: name.to_string(),
            f,
        }
    }
}

impl<F: FnMut(&mut Scene) -> CmdResult> Command for FnCommand<F> {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        (self.f)(scene)
    }
}

/// A list of commands executed and reverted as one atomic step.
#[derive(Default)]
pub struct CompoundCommand {
    pub commands: Vec<Box<dyn Command>>,
}

impl CompoundCommand {
    pub fn new(commands: Vec<Box<dyn Command>>) -> Self {
        CompoundCommand { commands }
    }
}

impl Command for CompoundCommand {
    fn name(&self) -> &str {
        "CompoundCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for c in self.commands.iter_mut() {
            c.apply(scene)?;
        }
        Ok(())
    }

    fn hides(&self) -> Vec<crate::entity::EntityRef> {
        self.commands.iter().flat_map(|c| c.hides()).collect()
    }
}
