// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Layers (`LayersPanel`): one row per layer with its visibility and lock;
//! add, delete, purge, and move the selection onto the highlighted layer.

use xtrude_model::commands::{assign_layer, layer_of, AssignLayerCommand, PurgeUnusedCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::layers::{Layer, DEFAULT_LAYER};
use xtrude_model::purge::iter_meshes;

use crate::window::MainWindow;

/// One layer row.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// The default layer cannot be renamed nor deleted.
    pub editable: bool,
}

#[derive(Default)]
pub struct Layers {
    pub rows: Vec<LayerRow>,
    /// The highlighted row.
    pub current: Option<String>,
}

impl Layers {
    pub fn refresh(&mut self, w: &MainWindow) {
        self.rows = w
            .viewport
            .scene
            .doc
            .layers
            .iter()
            .map(|l| LayerRow {
                name: l.name.clone(),
                visible: l.visible,
                locked: l.locked,
                editable: l.name != DEFAULT_LAYER,
            })
            .collect();
        if let Some(c) = &self.current {
            if !self.rows.iter().any(|r| &r.name == c) {
                self.current = None;
            }
        }
    }

    fn touch(w: &mut MainWindow) {
        w.viewport.scene.version += 1;
        w.viewport.update();
        w.pump_events();
    }

    /// The Visible / Lock checks of `name` changed.
    pub fn set_state(&mut self, w: &mut MainWindow, name: &str, visible: bool, locked: bool) {
        let Some(ly) = w.viewport.scene.doc.layer_mut(name) else {
            return;
        };
        ly.visible = visible;
        ly.locked = locked;
        if !visible || locked {
            self.prune_selection(w, name);
        }
        Self::touch(w);
        self.refresh(w);
    }

    /// A row renamed in place: the entities follow.
    pub fn rename(&mut self, w: &mut MainWindow, old: &str, new: &str) {
        let new = new.trim();
        let scene = &mut w.viewport.scene;
        if new.is_empty() || new == old || old == DEFAULT_LAYER || scene.doc.layer(new).is_some() {
            self.refresh(w);
            return;
        }
        if let Some(ly) = scene.doc.layer_mut(old) {
            ly.name = new.to_string();
        }
        let mut ents: Vec<EntityRef> = Vec::new();
        let mid = scene.active_mesh_id();
        let m = scene.mesh();
        ents.extend(m.face_ids().into_iter().map(|f| EntityRef::Face(mid, f)));
        ents.extend(m.edge_ids().into_iter().map(|e| EntityRef::Edge(mid, e)));
        ents.extend(scene.doc.groups.iter().map(|g| EntityRef::Group(g.id)));
        for e in ents {
            if scene.entity_layer(&e).as_deref() == Some(old) {
                assign_layer(scene, &e, Some(new.to_string()));
            }
        }
        if self.current.as_deref() == Some(old) {
            self.current = Some(new.to_string());
        }
        self.refresh(w);
        Self::touch(w);
    }

    fn prune_selection(&mut self, w: &mut MainWindow, name: &str) {
        let scene = &mut w.viewport.scene;
        let dead: Vec<EntityRef> = scene
            .selection
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EntityRef::Face(..) | EntityRef::Edge(..) | EntityRef::Group(_)
                )
            })
            .filter(|e| layer_of(scene, e) == name)
            .copied()
            .collect();
        for e in dead {
            scene.discard(&e);
        }
    }

    /// «+ Layer»: «Layer N», the first number free.
    pub fn add(&mut self, w: &mut MainWindow) {
        let scene = &mut w.viewport.scene;
        let base = tr("Layer", &[]);
        let mut n = 1;
        while scene.doc.layer(&format!("{base} {n}")).is_some() {
            n += 1;
        }
        scene.doc.layers.push(Layer::new(&format!("{base} {n}")));
        self.refresh(w);
        Self::touch(w);
    }

    /// «−»: the highlighted layer goes, its entities to the default.
    pub fn delete(&mut self, w: &mut MainWindow) {
        let Some(name) = self.current.clone() else {
            return;
        };
        if name == DEFAULT_LAYER {
            return;
        }
        let scene = &mut w.viewport.scene;
        if scene.doc.layer(&name).is_none() {
            return;
        }
        let mut ents: Vec<EntityRef> = Vec::new();
        for mid in iter_meshes(scene) {
            let m = scene.doc.mesh(mid);
            ents.extend(m.face_ids().into_iter().map(|f| EntityRef::Face(mid, f)));
            ents.extend(m.edge_ids().into_iter().map(|e| EntityRef::Edge(mid, e)));
        }
        ents.extend(
            scene
                .doc
                .all_groups()
                .iter()
                .map(|g| EntityRef::Group(g.id)),
        );
        ents.extend(
            scene
                .doc
                .dimensions
                .iter()
                .map(|d| EntityRef::Dimension(d.id)),
        );
        ents.extend(
            scene
                .doc
                .text_labels
                .iter()
                .map(|t| EntityRef::TextLabel(t.id)),
        );
        for e in ents {
            if layer_of(scene, &e) == name {
                assign_layer(scene, &e, None);
            }
        }
        scene.doc.layers.retain(|l| l.name != name);
        self.current = None;
        self.refresh(w);
        Self::touch(w);
    }

    /// «Purge»: the layers nothing carries, one undo step — and it says
    /// what it did.
    pub fn purge(&mut self, w: &mut MainWindow) {
        let mut cmd = PurgeUnusedCommand::new(true, false);
        let (names, _) = cmd.names(&w.viewport.scene);
        if names.is_empty() {
            w.dialogs
                .warn(&tr("Purge", &[]), &tr("Every layer is in use.", &[]));
            return;
        }
        let n = names.len();
        let mut preview = names
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if n > 12 {
            preview.push('…');
        }
        let q = format!(
            "{}\n\n{preview}",
            tr("Delete {n} layers no entity carries?", &[("n", &n)])
        );
        if !w.dialogs.confirm(&tr("Purge", &[]), &q) {
            return;
        }
        w.viewport.execute(&mut cmd);
        self.refresh(w);
        w.viewport.update();
        w.show_message(&tr("{n} unused layers purged", &[("n", &n)]), 3000);
    }

    /// «Assign selection»: onto the highlighted layer, saying so when
    /// there is nothing to move or no layer picked.
    pub fn assign(&mut self, w: &mut MainWindow) {
        let Some(name) = self.current.clone() else {
            w.show_message(
                &tr("Click a layer in the list first, then Assign.", &[]),
                3000,
            );
            return;
        };
        let targets: Vec<EntityRef> = w
            .viewport
            .scene
            .selection
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EntityRef::Face(..)
                        | EntityRef::Edge(..)
                        | EntityRef::Group(_)
                        | EntityRef::Dimension(_)
                        | EntityRef::TextLabel(_)
                )
            })
            .copied()
            .collect();
        if targets.is_empty() {
            w.show_message(
                &tr("Select something in the model first, then Assign.", &[]),
                3000,
            );
            return;
        }
        let n = targets.len();
        w.viewport.execute(&mut AssignLayerCommand {
            entities: targets,
            layer: name.clone(),
        });
        w.viewport.update();
        w.show_message(
            &tr(
                "{n} entities moved to '{layer}'",
                &[("n", &n), ("layer", &name)],
            ),
            2500,
        );
    }
}
