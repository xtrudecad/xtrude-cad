// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Parts (`PartsPanel`): the parts of the selected component, one level
//! deep, measured like a cut list — click selects inside the component,
//! the check shows or hides, the name edits in place; Split into Pieces
//! and the exploded view.

use glam::DVec3;
use xtrude_model::commands::{ExplodeViewCommand, HideCommand, RenameGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::Group;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::parts::{is_surface, part_rows};
use xtrude_model::units::{fmt_len_fine, fmt_triple};

use crate::window::MainWindow;

/// The exploded view's directions.
pub const EXPLODE_MODES: [(&str, &str); 4] = [
    ("outward", "Outward"),
    ("z", "Along blue (up)"),
    ("x", "Along red"),
    ("y", "Along green"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct PartLine {
    pub part: GroupId,
    pub name: String,
    pub size: String,
    pub size_tip: String,
    pub color: String,
    pub faces: usize,
    pub hidden: bool,
    pub selected: bool,
}

/// The «All parts visible» check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllVisible {
    #[default]
    Checked,
    Unchecked,
    Partial,
}

#[derive(Default)]
pub struct Parts {
    pub container: Option<GroupId>,
    pub title: String,
    pub lines: Vec<PartLine>,
    pub all_visible: AllVisible,
    /// The exploded view: slider (percent) and direction.
    pub explode_percent: i32,
    pub explode_mode: String,
    pub reassemble_enabled: bool,
    pub split_enabled: bool,
    /// A rename in progress: the part and the text typed so far. A refresh
    /// leaves the panel alone while it lasts.
    pub editing: Option<(GroupId, String)>,
    /// The row to scroll into view, taken by the shell when it draws it.
    pub scroll_to: Option<GroupId>,
    /// The selected parts the list last showed.
    shown_selection: Vec<GroupId>,
    drag: Option<(GroupId, Group, Vec<DVec3>)>,
}

impl Parts {
    /// The component whose parts are listed (`container`).
    pub fn container_of(w: &MainWindow) -> Option<GroupId> {
        let scene = &w.viewport.scene;
        let groups: Vec<GroupId> = scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::Group(g) => Some(*g),
                _ => None,
            })
            .collect();
        let has_kids = |g: GroupId| {
            scene
                .doc
                .find_group(g)
                .is_some_and(|x| !x.children.is_empty())
        };
        if groups.len() == 1 && has_kids(groups[0]) {
            return Some(groups[0]);
        }
        if let Some(e) = scene.edit_group() {
            if has_kids(e) {
                return Some(e);
            }
        }
        (groups.len() == 1).then(|| groups[0])
    }

    /// Bring the list up to date — except while a rename is being typed,
    /// which a refresh would kill.
    pub fn refresh(&mut self, w: &MainWindow) {
        if self.editing.is_some() {
            return; // a rename in progress: leave it be
        }
        let cont = Self::container_of(w);
        self.container = cont;
        let doc = &w.viewport.scene.doc;
        let sel = &w.viewport.scene.selection;
        let g = cont.and_then(|c| doc.find_group(c));
        let rows = g.map(|g| part_rows(&doc.meshes, g)).unwrap_or_default();
        self.lines = rows
            .iter()
            .map(|r| {
                let (size, tip) = if is_surface(r.size) {
                    (
                        tr(
                            "{area} — surface",
                            &[("area", &format!("{} × {}", fmt_len_fine(r.size.0), fmt_len_fine(r.size.1)))],
                        ),
                        tr(
                            "No thickness: a surface drawn as a single plane (a skin, a decal, a glass pane). It stays in the model.",
                            &[],
                        ),
                    )
                } else {
                    (fmt_triple(r.size.0, r.size.1, r.size.2, true), String::new())
                };
                PartLine {
                    part: r.part,
                    name: doc.find_group(r.part).map(|p| p.name.clone()).unwrap_or_else(|| r.name.clone()),
                    size,
                    size_tip: tip,
                    color: r.material.clone(),
                    faces: r.faces,
                    hidden: doc.find_group(r.part).is_some_and(|p| p.hidden),
                    selected: sel.contains(&EntityRef::Group(r.part)),
                }
            })
            .collect();
        self.sync_selection();
        let hidden = self.lines.iter().filter(|l| l.hidden).count();
        self.all_visible = if hidden == 0 {
            AllVisible::Checked
        } else if hidden == self.lines.len() {
            AllVisible::Unchecked
        } else {
            AllVisible::Partial
        };
        self.title = match g {
            None => tr("Select a component to see its parts.", &[]),
            Some(g) if self.lines.is_empty() => tr(
                "«{name}» is one piece. Split into Pieces finds the solids inside it that do not touch.",
                &[("name", &g.name)],
            ),
            Some(g) => tr("<b>{name}</b> — {n} parts", &[("name", &g.name), ("n", &self.lines.len())]),
        };
        if self.drag.is_none() {
            let state = g.and_then(|g| g.exploded.clone());
            self.explode_percent = state
                .as_ref()
                .map_or(0, |s| (s.factor * 100.0).round() as i32);
            self.explode_mode = state.as_ref().map_or("outward".into(), |s| s.mode.clone());
        }
        self.reassemble_enabled = g.is_some_and(|g| g.exploded.is_some());
        self.split_enabled = cont.is_some();
    }

    /// When the selection changed, scroll the first
    /// selected part into view, so a part clicked in the canvas is found in
    /// the list without hunting.
    fn sync_selection(&mut self) {
        let chosen = self.selected_parts();
        if !chosen.is_empty() && chosen != self.shown_selection {
            self.scroll_to = Some(chosen[0]);
        }
        self.shown_selection = chosen;
    }

    fn selected_parts(&self) -> Vec<GroupId> {
        self.lines
            .iter()
            .filter(|l| l.selected)
            .map(|l| l.part)
            .collect()
    }

    fn after(&mut self, w: &mut MainWindow) {
        w.viewport.update();
        w.pump_events();
        self.refresh(w);
    }

    /// «All parts visible»: show every part, or hide them all when all show.
    pub fn toggle_all_visible(&mut self, w: &mut MainWindow) {
        let hidden: Vec<EntityRef> = self
            .lines
            .iter()
            .filter(|l| l.hidden)
            .map(|l| EntityRef::Group(l.part))
            .collect();
        let mut cmd = if !hidden.is_empty() {
            HideCommand {
                entities: hidden,
                hidden: false,
            }
        } else if !self.lines.is_empty() {
            HideCommand {
                entities: self
                    .lines
                    .iter()
                    .map(|l| EntityRef::Group(l.part))
                    .collect(),
                hidden: true,
            }
        } else {
            return;
        };
        w.viewport.execute(&mut cmd);
        self.after(w);
    }

    /// The row's check: show or hide one part.
    pub fn set_hidden(&mut self, w: &mut MainWindow, part: GroupId, hidden: bool) {
        w.viewport.execute(&mut HideCommand {
            entities: vec![EntityRef::Group(part)],
            hidden,
        });
        self.after(w);
    }

    /// Double-click, F2 or Rename…: start editing `part`'s name in place.
    pub fn begin_rename(&mut self, part: GroupId) {
        if let Some(l) = self.lines.iter().find(|l| l.part == part) {
            self.editing = Some((part, l.name.clone()));
        }
    }

    /// The name editor finished (Enter / focus out): rename to its text.
    pub fn commit_rename(&mut self, w: &mut MainWindow) {
        let Some((part, text)) = self.editing.take() else {
            return;
        };
        self.rename(w, part, &text);
        self.refresh(w);
    }

    pub fn rename(&mut self, w: &mut MainWindow, part: GroupId, text: &str) {
        let name = text.trim();
        if name.is_empty()
            || w.viewport
                .scene
                .doc
                .find_group(part)
                .is_none_or(|g| g.name == name)
        {
            return;
        }
        w.viewport.execute(&mut RenameGroupCommand {
            group: part,
            name: name.to_string(),
        });
        self.after(w);
    }

    /// Click (Ctrl/Shift: several): open the component, select the parts.
    pub fn click(&mut self, w: &mut MainWindow, parts: &[GroupId]) {
        let Some(cont) = self.container else { return };
        let parts: Vec<GroupId> = parts
            .iter()
            .copied()
            .filter(|p| self.lines.iter().any(|l| l.part == *p && !l.hidden))
            .collect();
        if parts.is_empty() {
            return;
        }
        if w.viewport.scene.edit_group() != Some(cont) {
            w.viewport.begin_group_edit(cont);
        }
        let scene = &mut w.viewport.scene;
        scene.selection.clear();
        for p in parts {
            scene.selection.insert(EntityRef::Group(p));
        }
        // Clicked here, so already in view: the refresh this triggers must
        // not scroll the list under the pointer.
        self.shown_selection = self
            .lines
            .iter()
            .filter(|l| scene.selection.contains(&EntityRef::Group(l.part)))
            .map(|l| l.part)
            .collect();
        scene.bump_view();
        self.after(w);
    }

    /// «Split into Pieces»: on the closed component.
    pub fn split(&mut self, w: &mut MainWindow) {
        let Some(cont) = self.container else { return };
        if w.viewport.scene.edit_group() == Some(cont) {
            w.viewport.end_group_edit();
        }
        let scene = &mut w.viewport.scene;
        scene.selection.clear();
        scene.selection.insert(EntityRef::Group(cont));
        w.trigger("split_into_pieces");
        self.after(w);
    }

    /// The container the slider drives, closed first.
    fn explode_target(&mut self, w: &mut MainWindow) -> Option<GroupId> {
        let cont = self.container?;
        if w.viewport
            .scene
            .doc
            .find_group(cont)
            .is_none_or(|g| g.children.len() < 2)
        {
            return None;
        }
        if w.viewport.scene.edit_group() == Some(cont) {
            w.viewport.end_group_edit();
            let scene = &mut w.viewport.scene;
            scene.selection.clear();
            scene.selection.insert(EntityRef::Group(cont));
        }
        Some(cont)
    }

    fn run_explode(&mut self, w: &mut MainWindow, factor: f64) {
        let Some(cont) = self.explode_target(w) else {
            return;
        };
        w.viewport.execute(&mut ExplodeViewCommand {
            container: cont,
            factor,
            mode: self.explode_mode.clone(),
        });
        self.after(w);
    }

    /// The slider pressed: a live preview until it is released.
    pub fn explode_press(&mut self, w: &mut MainWindow) {
        let Some(cont) = self.explode_target(w) else {
            return;
        };
        let doc = &w.viewport.scene.doc;
        let Some(g) = doc.find_group(cont) else {
            return;
        };
        let centres = xtrude_model::explode::assembled_centres(&doc.meshes, g);
        self.drag = Some((cont, g.clone(), centres));
    }

    /// The slider moved to `percent`.
    pub fn explode_value(&mut self, w: &mut MainWindow, percent: i32) {
        self.explode_percent = percent;
        let Some((cont, _, centres)) = self.drag.clone() else {
            // a click on the track or an arrow key: one step
            self.run_explode(w, percent as f64 / 100.0);
            return;
        };
        let scene = &mut w.viewport.scene;
        let meshes = scene.doc.meshes.clone();
        if let Some(g) = scene.doc.find_group_mut(cont) {
            xtrude_model::explode::apply_explode(
                &meshes,
                g,
                percent as f64 / 100.0,
                &self.explode_mode,
                Some(centres),
            );
        }
        scene.version += 1;
        w.viewport.update();
    }

    /// The slider released: the preview undone, one command lands.
    pub fn explode_release(&mut self, w: &mut MainWindow) {
        let Some((cont, snap, _)) = self.drag.take() else {
            return;
        };
        if let Some(g) = w.viewport.scene.doc.find_group_mut(cont) {
            *g = snap;
        }
        self.run_explode(w, self.explode_percent as f64 / 100.0);
    }

    pub fn explode_mode_changed(&mut self, w: &mut MainWindow, mode: &str) {
        self.explode_mode = mode.to_string();
        let exploded = self
            .container
            .and_then(|c| w.viewport.scene.doc.find_group(c))
            .is_some_and(|g| g.exploded.is_some());
        if exploded {
            self.run_explode(w, self.explode_percent as f64 / 100.0);
        }
    }

    pub fn reassemble(&mut self, w: &mut MainWindow) {
        self.run_explode(w, 0.0);
    }
}
