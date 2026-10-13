// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The exploded view (`ExplodedView`), a drop-down panel on the View bar:
//! the parts of the selected group drawn apart along a direction, by a
//! slider that previews live and lands as one undo step; Reassemble puts
//! them back.

use glam::DVec3;
use xtrude_model::commands::ExplodeViewCommand;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::Group;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;

use crate::window::MainWindow;

/// The exploded view's directions.
pub const EXPLODE_MODES: [(&str, &str); 4] = [
    ("outward", "Outward"),
    ("z", "Along blue (up)"),
    ("x", "Along red"),
    ("y", "Along green"),
];

#[derive(Default)]
pub struct ExplodedView {
    /// The group whose parts move apart.
    pub container: Option<GroupId>,
    /// What the panel says about it.
    pub title: String,
    /// Whether the slider applies: a group of two parts or more.
    pub enabled: bool,
    /// The slider (percent) and the direction.
    pub explode_percent: i32,
    pub explode_mode: String,
    pub reassemble_enabled: bool,
    drag: Option<(GroupId, Group, Vec<DVec3>)>,
}

impl ExplodedView {
    /// The group the view drives: the one selected group with parts, else
    /// the open group with parts, else the one selected group.
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

    pub fn refresh(&mut self, w: &MainWindow) {
        let cont = Self::container_of(w);
        self.container = cont;
        let g = cont.and_then(|c| w.viewport.scene.doc.find_group(c));
        let parts = g.map_or(0, |g| g.children.len());
        self.enabled = parts >= 2;
        self.title = match g {
            None => tr("Select a group to draw its parts apart.", &[]),
            Some(g) if parts < 2 => tr(
                "«{name}» has fewer than two parts. Split into Pieces finds the solids inside it that do not touch.",
                &[("name", &g.name)],
            ),
            Some(g) => tr(
                "<b>{name}</b> — {n} parts",
                &[("name", &g.name), ("n", &parts)],
            ),
        };
        if self.drag.is_none() {
            let state = g.and_then(|g| g.exploded.clone());
            self.explode_percent = state
                .as_ref()
                .map_or(0, |s| (s.factor * 100.0).round() as i32);
            self.explode_mode = state.as_ref().map_or("outward".into(), |s| s.mode.clone());
        }
        self.reassemble_enabled = g.is_some_and(|g| g.exploded.is_some());
    }

    fn after(&mut self, w: &mut MainWindow) {
        w.viewport.update();
        w.pump_events();
        self.refresh(w);
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
