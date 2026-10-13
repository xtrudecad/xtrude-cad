// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The model tree (`ModelTree`): the model as a tree of its groups and
//! components — click selects (opening the groups above it), double-click
//! edits, a rename is one undo step, the eye shows or hides, and dropping
//! rows on a group moves them there without moving them in the world.
//!
//! It is where the model's structure is read: a component row counts its
//! copies and selects them all; «Components only» lists each definition
//! once ([`Components`]); optional columns measure each row (size along
//! the model axes, faces, colour); and each top-level row is marked as
//! the link a robot description makes of it, with a warning when the
//! link's name is not the group's ([`xtrude_model::links`]).

use std::collections::{HashMap, HashSet};

use xtrude_model::commands::{HideCommand, RenameGroupCommand, ReparentGroupsCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{iter_placements, Group};
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::links::links;
use xtrude_model::model_tree::{ancestors, can_reparent, parent_map};
use xtrude_model::parts::part_material;
use xtrude_model::settings::{self, keys};
use xtrude_model::units::fmt_triple;

use super::components::Components;
use super::natural_key;
use crate::window::MainWindow;

/// The optional columns: `(key, title)`, in their order on screen. The key
/// is what `ui/model_tree_columns` stores.
pub const COLUMNS: [(&str, &str); 3] = [("size", "Size"), ("faces", "Faces"), ("color", "Color")];

/// One row of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub group: GroupId,
    pub name: String,
    pub component: bool,
    pub hidden: bool,
    /// A component's copies in the model, at every level; 1 for a group.
    pub copies: usize,
    /// The link the row's top-level group makes (its own name for a
    /// top-level row, the one it merges into for a nested one).
    pub link: String,
    /// Whether the row is a top-level group: a link of its own.
    pub top_level: bool,
    /// The optional columns, `""` while they are off: W × D × H along the
    /// model axes, the faces counted with the nested groups', the colour
    /// most of its surface wears.
    pub size: String,
    pub faces: String,
    pub color: String,
    pub children: Vec<Row>,
}

impl Row {
    /// The link mark's warning, for a top-level row whose link does not
    /// go out under the group's name.
    pub fn link_warning(&self) -> Option<String> {
        (self.top_level && self.link != self.name).then(|| {
            tr(
                "Exports as «{link}»: a link name holds only letters, digits and _, and no two are the same",
                &[("link", &self.link)],
            )
        })
    }

    /// What the link mark's tooltip says.
    pub fn link_tip(&self) -> String {
        if self.top_level {
            tr(
                "Link «{link}» of a robot description",
                &[("link", &self.link)],
            )
        } else {
            tr("Merged into link «{link}»", &[("link", &self.link)])
        }
    }

    /// The text of the optional column `key`.
    pub fn column(&self, key: &str) -> &str {
        match key {
            "size" => &self.size,
            "faces" => &self.faces,
            _ => &self.color,
        }
    }
}

#[derive(Default)]
pub struct ModelTree {
    pub filter: String,
    /// «Components only»: one row per component definition
    /// ([`Self::components`]) instead of the tree.
    pub components_only: bool,
    /// The component definitions and their copies.
    pub components: Components,
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
    /// The row to scroll into view, taken by the shell when it draws it:
    /// the first group selected in the canvas.
    pub scroll_to: Option<GroupId>,
    signature: Option<String>,
}

/// What a row is built from besides its group.
struct Build<'a> {
    w: &'a MainWindow,
    copies: HashMap<GroupId, usize>,
    columns: Vec<&'static str>,
}

impl Build<'_> {
    /// The rows of `nodes`, under the link `link` (`None` at the top
    /// level, where each group is a link of its own, named in `links`).
    fn rows(
        &self,
        nodes: &[Group],
        link: Option<&str>,
        links: &HashMap<GroupId, String>,
    ) -> Vec<Row> {
        let mut rows: Vec<Row> = nodes
            .iter()
            .map(|g| {
                let top_level = link.is_none();
                let link = link
                    .map(str::to_string)
                    .or_else(|| links.get(&g.id).cloned())
                    .unwrap_or_default();
                Row {
                    group: g.id,
                    name: g.name.clone(),
                    component: g.is_component(),
                    hidden: g.hidden,
                    copies: self.copies.get(&g.id).copied().unwrap_or(1),
                    top_level,
                    size: self.column("size", || self.size(g)),
                    faces: self.column("faces", || self.faces(g)),
                    color: self.column("color", || {
                        part_material(&self.w.viewport.scene.doc.meshes, g)
                    }),
                    children: self.rows(&g.children, Some(&link), links),
                    link,
                }
            })
            .collect();
        rows.sort_by_key(|r| natural_key(&r.name));
        rows
    }

    fn column(&self, key: &str, text: impl FnOnce() -> String) -> String {
        if self.columns.contains(&key) {
            text()
        } else {
            String::new()
        }
    }

    fn size(&self, g: &Group) -> String {
        let pts = self.w.viewport.scene.doc.group_world_points(g.id);
        if pts.is_empty() {
            return "—".into();
        }
        let (lo, hi) = pts
            .iter()
            .fold((pts[0], pts[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let d = hi - lo;
        fmt_triple(d.x, d.y, d.z, false)
    }

    fn faces(&self, g: &Group) -> String {
        let doc = &self.w.viewport.scene.doc;
        iter_placements(g, None)
            .iter()
            .map(|(p, _)| doc.mesh(p.mesh).faces.len())
            .sum::<usize>()
            .to_string()
    }
}

/// The optional columns shown, in [`COLUMNS`] order
/// (`ui/model_tree_columns`; none by default).
pub fn shown_columns() -> Vec<&'static str> {
    let stored = settings::read(&keys::MODEL_TREE_COLUMNS);
    let on = |k: &str| {
        stored
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(k)))
    };
    COLUMNS.iter().map(|(k, _)| *k).filter(|k| on(k)).collect()
}

/// Show or hide the optional column `key`, and remember it.
pub fn set_column_shown(key: &str, shown: bool) {
    let mut cols = shown_columns();
    cols.retain(|k| *k != key);
    if shown {
        cols.extend(COLUMNS.iter().map(|(k, _)| *k).find(|k| *k == key));
    }
    let ordered: Vec<&str> = COLUMNS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| cols.contains(k))
        .collect();
    settings::write(&keys::MODEL_TREE_COLUMNS, &serde_json::json!(ordered));
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

    fn signature(w: &MainWindow, columns: &[&str], copies: &HashMap<GroupId, usize>) -> String {
        let mut s = Self::document_name(w);
        // the columns measure the geometry: any edit may change them
        if !columns.is_empty() {
            s.push_str(&format!(
                "|{}:{}",
                columns.join(","),
                w.viewport.scene.version
            ));
        }
        let mut c: Vec<_> = copies.iter().collect();
        c.sort();
        s.push_str(&format!("|{c:?}"));
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
        self.components.refresh_in_model(w);
        let copies: HashMap<GroupId, usize> = self
            .components
            .rows
            .iter()
            .flat_map(|r| r.groups.iter().map(|g| (*g, r.groups.len())))
            .collect();
        let columns = shown_columns();
        let sig = Self::signature(w, &columns, &copies);
        if self.signature.as_deref() != Some(sig.as_str()) {
            self.signature = Some(sig);
            self.root_name = Self::document_name(w);
            let links: HashMap<GroupId, String> = links(&w.viewport.scene.doc)
                .into_iter()
                .map(|l| (l.group, l.name))
                .collect();
            let build = Build { w, copies, columns };
            self.rows = build.rows(&w.viewport.scene.doc.groups, None, &links);
        }
        self.sync_from_scene(w);
    }

    /// Show or hide the optional column `key`; the rows are measured again.
    pub fn set_column_shown(&mut self, w: &MainWindow, key: &str, shown: bool) {
        set_column_shown(key, shown);
        self.signature = None;
        self.refresh(w);
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
            // picked in the canvas: found in the tree without hunting
            self.scroll_to = wanted.first().copied();
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

    /// Select all copies: every copy of `group`'s component that shares
    /// its parent (the ones at other levels are not selectable with it).
    pub fn select_copies(&mut self, w: &mut MainWindow, group: GroupId) {
        let Some(row) = self
            .components
            .rows
            .iter()
            .position(|r| r.groups.contains(&group))
        else {
            return;
        };
        self.components.select_at_level_of(w, row, group);
        self.refresh(w);
    }

    /// «Components only»: a definition's row clicked selects its copies,
    /// those at the first copy's level.
    pub fn select_definition(&mut self, w: &mut MainWindow, row: usize) {
        self.components.select(w, row);
        self.refresh(w);
    }

    /// Split into Pieces on `group`, selected alone and closed.
    pub fn split(&mut self, w: &mut MainWindow, group: GroupId) {
        if w.viewport.scene.edit_group() == Some(group) {
            w.viewport.end_group_edit();
        }
        super::select_groups(w, &[group]);
        w.trigger("split_into_pieces");
        self.expanded.insert(group);
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
