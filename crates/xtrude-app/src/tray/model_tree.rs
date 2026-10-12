// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The model tree (`ModelTree`): the model as a tree of its groups and
//! components — click selects (opening the groups above it), double-click
//! edits, a rename is one undo step, and dropping rows on a group moves
//! them there without moving them in the world.

use std::collections::{HashMap, HashSet};

use xtrude_model::commands::{HideCommand, RenameGroupCommand, ReparentGroupsCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::Group;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::model_tree::{ancestors, can_reparent, parent_map};

use super::natural_key;
use crate::window::MainWindow;

/// One row of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub group: GroupId,
    pub name: String,
    pub component: bool,
    pub hidden: bool,
    pub children: Vec<Row>,
}

#[derive(Default)]
pub struct ModelTree {
    pub filter: String,
    /// The document's name (the root row).
    pub root_name: String,
    pub rows: Vec<Row>,
    /// Rows open in the tree.
    pub expanded: HashSet<GroupId>,
    /// Rows selected in the tree (the root row is `root_selected`).
    pub selected: Vec<GroupId>,
    pub root_selected: bool,
    /// The groups open for editing (shown bold).
    pub open: Vec<GroupId>,
    signature: Option<String>,
}

fn build(nodes: &[Group]) -> Vec<Row> {
    let mut rows: Vec<Row> = nodes
        .iter()
        .map(|g| Row {
            group: g.id,
            name: g.name.clone(),
            component: g.is_component(),
            hidden: g.hidden,
            children: build(&g.children),
        })
        .collect();
    rows.sort_by_key(|r| natural_key(&r.name));
    rows
}

impl ModelTree {
    fn document_name(w: &MainWindow) -> String {
        if let Some(p) = &w.current_path {
            return p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        if let Some(n) = &w.import_name {
            return std::path::Path::new(n)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        tr("Model", &[])
    }

    fn signature(w: &MainWindow) -> String {
        let mut s = Self::document_name(w);
        fn walk(nodes: &[Group], depth: usize, s: &mut String) {
            for g in nodes {
                s.push_str(&format!(
                    "|{:?}:{depth}:{}:{}:{}",
                    g.id,
                    g.name,
                    g.hidden,
                    g.is_component()
                ));
                walk(&g.children, depth + 1, s);
            }
        }
        walk(&w.viewport.scene.doc.groups, 0, &mut s);
        s
    }

    pub fn refresh(&mut self, w: &MainWindow) {
        let sig = Self::signature(w);
        if self.signature.as_deref() != Some(sig.as_str()) {
            self.signature = Some(sig);
            self.root_name = Self::document_name(w);
            self.rows = build(&w.viewport.scene.doc.groups);
        }
        self.sync_from_scene(w);
    }

    /// The viewport's selection highlighted, and the open groups.
    fn sync_from_scene(&mut self, w: &MainWindow) {
        let scene = &w.viewport.scene;
        self.open = scene.edit_stack.iter().map(|l| l.group).collect();
        let known = parent_map(scene);
        let wanted: Vec<GroupId> = scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::Group(g) if known.contains_key(g) => Some(*g),
                _ => None,
            })
            .collect();
        let mut a = wanted.clone();
        a.sort();
        let mut b = self.selected.clone();
        b.sort();
        if a != b {
            self.selected = wanted.clone();
            self.root_selected = false;
            for g in wanted {
                for p in ancestors(&known, g) {
                    self.expanded.insert(p);
                }
            }
        }
    }

    /// The row of `g`, if any.
    pub fn row(&self, g: GroupId) -> Option<&Row> {
        fn find(rows: &[Row], g: GroupId) -> Option<&Row> {
            for r in rows {
                if r.group == g {
                    return Some(r);
                }
                if let Some(x) = find(&r.children, g) {
                    return Some(x);
                }
            }
            None
        }
        find(&self.rows, g)
    }

    /// The tree as indented names (the rows a test reads).
    pub fn lines(&self) -> Vec<String> {
        fn walk(rows: &[Row], depth: usize, out: &mut Vec<String>) {
            for r in rows {
                out.push(format!("{}{}", "  ".repeat(depth), r.name));
                walk(&r.children, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.rows, 0, &mut out);
        out
    }

    /// Whether a row shows — it matches, or a row below
    /// it does.
    pub fn shown(&self, g: GroupId) -> bool {
        let needle = self.filter.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        fn keep(r: &Row, needle: &str) -> bool {
            r.name.to_lowercase().contains(needle) || r.children.iter().any(|c| keep(c, needle))
        }
        self.row(g).is_some_and(|r| keep(r, &needle))
    }

    fn parent_row(&self, g: GroupId) -> Option<GroupId> {
        fn find(rows: &[Row], g: GroupId, parent: Option<GroupId>) -> Option<Option<GroupId>> {
            for r in rows {
                if r.group == g {
                    return Some(parent);
                }
                if let Some(x) = find(&r.children, g, Some(r.group)) {
                    return Some(x);
                }
            }
            None
        }
        find(&self.rows, g, None).flatten()
    }

    /// Select `groups` in the model, opening their parents.
    pub fn select_groups(&mut self, w: &mut MainWindow, groups: &[GroupId]) {
        super::select_groups(w, groups);
        self.refresh(w);
    }

    /// The tree's selection changed: `current` is
    /// the row clicked, `root` whether the model row is among them.
    pub fn tree_selection(
        &mut self,
        w: &mut MainWindow,
        rows: &[GroupId],
        current: Option<GroupId>,
        root: bool,
    ) {
        if root {
            w.viewport.end_group_edit();
            w.viewport.scene.selection.clear();
            w.viewport.scene.bump_view();
            w.viewport.update();
            w.pump_events();
            self.selected.clear();
            self.root_selected = true;
            self.refresh(w);
            return;
        }
        if rows.is_empty() {
            return;
        }
        let anchor = current.filter(|c| rows.contains(c)).unwrap_or(rows[0]);
        let same = self.parent_row(anchor);
        let groups: Vec<GroupId> = rows
            .iter()
            .copied()
            .filter(|g| self.parent_row(*g) == same)
            .collect();
        self.selected = groups.clone();
        self.root_selected = false;
        self.select_groups(w, &groups);
    }

    /// Double-click: open `group` for editing, its parents first.
    pub fn edit_group(&mut self, w: &mut MainWindow, group: GroupId) {
        super::edit_group(w, group);
        w.viewport.update();
        w.pump_events();
        self.refresh(w);
    }

    /// A row renamed in place: one undo step; an empty name is not taken.
    pub fn rename(&mut self, w: &mut MainWindow, group: GroupId, text: &str) {
        let name = text.trim();
        let current = w
            .viewport
            .scene
            .doc
            .find_group(group)
            .map(|g| g.name.clone());
        if name.is_empty() || current.as_deref() == Some(name) || current.is_none() {
            return;
        }
        w.viewport.execute(&mut RenameGroupCommand {
            group,
            name: name.to_string(),
        });
        w.pump_events();
        self.refresh(w);
    }

    pub fn set_hidden(&mut self, w: &mut MainWindow, groups: &[GroupId], hidden: bool) {
        let ents: Vec<EntityRef> = groups
            .iter()
            .filter(|g| {
                w.viewport
                    .scene
                    .doc
                    .find_group(**g)
                    .is_some_and(|x| x.hidden != hidden)
            })
            .map(|g| EntityRef::Group(*g))
            .collect();
        if ents.is_empty() {
            return;
        }
        w.viewport.execute(&mut HideCommand {
            entities: ents,
            hidden,
        });
        w.pump_events();
        self.refresh(w);
    }

    pub fn expand_all(&mut self) {
        fn walk(rows: &[Row], out: &mut HashSet<GroupId>) {
            for r in rows {
                if !r.children.is_empty() {
                    out.insert(r.group);
                }
                walk(&r.children, out);
            }
        }
        walk(&self.rows, &mut self.expanded);
    }

    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    /// The rows being dragged, minus those whose parent row goes along.
    fn dragged(&self, w: &MainWindow) -> Vec<GroupId> {
        let pm = parent_map(&w.viewport.scene);
        self.selected
            .iter()
            .copied()
            .filter(|g| !ancestors(&pm, *g).iter().any(|a| self.selected.contains(a)))
            .collect()
    }

    /// Whether the dragged rows may drop on `parent` (`None` = the model).
    pub fn can_drop(&self, w: &MainWindow, parent: Option<GroupId>) -> bool {
        can_reparent(&w.viewport.scene, &self.dragged(w), parent, true)
    }

    /// Move the dragged rows under `parent`, at `index` among its children
    /// (`None` = at the end). One undo step.
    pub fn drop(
        &mut self,
        w: &mut MainWindow,
        parent: Option<GroupId>,
        index: Option<usize>,
    ) -> bool {
        let groups = self.dragged(w);
        if groups.is_empty() {
            return false;
        }
        w.viewport.end_group_edit();
        if !can_reparent(&w.viewport.scene, &groups, parent, false) {
            w.viewport.flash_status(
                &tr(
                    "A group can't go inside itself, nor in or out of a component — Make Independent first",
                    &[],
                ),
                4000,
            );
            w.pump_events();
            self.refresh(w);
            return false;
        }
        w.viewport.execute(&mut ReparentGroupsCommand {
            groups,
            parent,
            index,
        });
        if let Some(p) = parent {
            self.expanded.insert(p);
        }
        w.pump_events();
        self.refresh(w);
        true
    }

    /// The rows' groups for the drag (`HashMap` of row → parent row) —
    /// the shell's drop targets.
    pub fn parents(&self) -> HashMap<GroupId, Option<GroupId>> {
        fn walk(
            rows: &[Row],
            parent: Option<GroupId>,
            out: &mut HashMap<GroupId, Option<GroupId>>,
        ) {
            for r in rows {
                out.insert(r.group, parent);
                walk(&r.children, Some(r.group), out);
            }
        }
        let mut out = HashMap::new();
        walk(&self.rows, None, &mut out);
        out
    }
}
