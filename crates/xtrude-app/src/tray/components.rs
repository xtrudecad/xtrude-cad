// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The component definitions placed in this drawing, one row per
//! definition, its copies counted wherever they are placed: the model
//! tree's copy counts, its «Select all copies» and its «Components only»
//! list (a row clicked selects the copies).

use std::collections::HashMap;

use xtrude_model::group::iter_placements;
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::model_tree::parent_map;

use crate::window::MainWindow;

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentRow {
    pub label: String,
    pub groups: Vec<GroupId>,
}

#[derive(Default)]
pub struct Components {
    pub rows: Vec<ComponentRow>,
}

impl Components {
    /// Per definition, most copies first. Every placement counts, nested
    /// in groups or in other components' definitions: each one is a group
    /// of its own, selectable inside its parent. An open copy counts with
    /// its definition, and the row shows the open copy's faces: the
    /// definition mesh takes the edits only on leaving.
    pub fn refresh_in_model(&mut self, w: &MainWindow) {
        let scene = &w.viewport.scene;
        let doc = &scene.doc;
        let mut by_proto: Vec<(MeshId, Vec<GroupId>)> = Vec::new();
        let mut index: HashMap<MeshId, usize> = HashMap::new();
        let placed = doc
            .groups
            .iter()
            .flat_map(|top| iter_placements(top, None))
            .map(|(g, _)| g);
        for g in placed {
            if !scene.is_component(g) {
                continue;
            }
            let mesh = scene.shared_mesh(g);
            match index.get(&mesh) {
                Some(&i) => by_proto[i].1.push(g.id),
                None => {
                    index.insert(mesh, by_proto.len());
                    by_proto.push((mesh, vec![g.id]));
                }
            }
        }
        let mut rows: Vec<(String, usize, Vec<GroupId>)> = by_proto
            .into_iter()
            .map(|(m, gs)| {
                let name = doc
                    .find_group(gs[0])
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                let live = scene
                    .edit_stack
                    .iter()
                    .find(|l| l.share.is_some() && gs.contains(&l.group))
                    .and_then(|l| doc.find_group(l.group))
                    .map_or(m, |g| g.mesh);
                (name, doc.mesh(live).face_ids().len(), gs)
            })
            .collect();
        rows.sort_by(|a, b| {
            b.2.len()
                .cmp(&a.2.len())
                .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        let faces_word = tr("Faces", &[]).to_lowercase();
        self.rows = rows
            .into_iter()
            .map(|(name, faces, gs)| ComponentRow {
                label: format!("{name}  ({}×, {faces} {faces_word})", gs.len()),
                groups: gs,
            })
            .collect();
    }

    /// The placeholder shown with no components.
    pub fn empty_label() -> String {
        tr("No components yet", &[])
    }

    /// Click a row: open the first copy's parents and select the copies
    /// that share its parent (those at other levels stay unselected).
    pub fn select(&mut self, w: &mut MainWindow, row: usize) {
        let Some(&first) = self.rows.get(row).and_then(|r| r.groups.first()) else {
            return;
        };
        self.select_at_level_of(w, row, first);
    }

    /// Select the copies of row `row` that share `copy`'s parent, opening
    /// that parent's chain.
    pub fn select_at_level_of(&mut self, w: &mut MainWindow, row: usize, copy: GroupId) {
        let Some(r) = self.rows.get(row) else { return };
        let parents = parent_map(&w.viewport.scene);
        let level = |g: &GroupId| parents.get(g).copied().flatten();
        let first = Some(level(&copy));
        let groups: Vec<GroupId> = r
            .groups
            .iter()
            .filter(|g| Some(level(g)) == first)
            .copied()
            .collect();
        super::select_groups(w, &groups);
    }
}
