// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The properties panel (`Properties`): facts about the selection, the name of
//! one group or component, and its layer — the one thing changed here.

use std::collections::{BTreeSet, HashMap};

use xtrude_model::commands::{layer_of, AssignLayerCommand, RenameGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::units::fmt_len;

use crate::window::MainWindow;

/// The layer field: `None` entry = «(several)».
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerField {
    pub visible: bool,
    /// `(label, data)`; data `None` for the mixed entry.
    pub items: Vec<(String, Option<String>)>,
    pub current: usize,
}

#[derive(Default)]
pub struct Properties {
    /// The description, as simple markup (`<b>…</b>`, `<br>`).
    pub text: String,
    /// The group the name field shows (one group selected).
    pub named: Option<GroupId>,
    pub name: String,
    pub layer: LayerField,
    solid_cache: Option<(u64, HashMap<GroupId, Option<f64>>)>,
}

fn taggable(e: &EntityRef) -> bool {
    matches!(
        e,
        EntityRef::Face(..)
            | EntityRef::Edge(..)
            | EntityRef::Group(_)
            | EntityRef::Dimension(_)
            | EntityRef::TextLabel(_)
    )
}

impl Properties {
    pub fn refresh(&mut self, w: &MainWindow) {
        let sel: Vec<EntityRef> = w.viewport.scene.selection.iter().copied().collect();
        self.text = self.describe(w, &sel);
        self.refresh_name(w, &sel);
        self.refresh_layer(w, &sel);
    }

    fn refresh_name(&mut self, w: &MainWindow, sel: &[EntityRef]) {
        let one = match sel {
            [EntityRef::Group(g)] => Some(*g),
            _ => None,
        };
        let changed = one != self.named;
        self.named = one;
        if let Some(g) = one {
            if changed {
                self.name = w
                    .viewport
                    .scene
                    .doc
                    .find_group(g)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
            }
        }
    }

    /// The name field finished editing (Enter / focus out).
    pub fn name_edited(&mut self, w: &mut MainWindow, text: &str) {
        let Some(g) = self.named else { return };
        let current = w
            .viewport
            .scene
            .doc
            .find_group(g)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let name = text.trim();
        if name.is_empty() || name == current {
            self.name = current; // empty: keep it
            return;
        }
        w.viewport.execute(&mut RenameGroupCommand {
            group: g,
            name: name.to_string(),
        });
        self.name = name.to_string();
        w.viewport.update();
    }

    fn refresh_layer(&mut self, w: &MainWindow, sel: &[EntityRef]) {
        let tagged: Vec<&EntityRef> = sel.iter().filter(|e| taggable(e)).collect();
        let scene = &w.viewport.scene;
        let mut f = LayerField {
            visible: !tagged.is_empty(),
            ..Default::default()
        };
        if f.visible {
            let current: BTreeSet<String> = tagged.iter().map(|e| layer_of(scene, e)).collect();
            if current.len() > 1 {
                f.items.push((tr("(several)", &[]), None));
            }
            for ly in &scene.doc.layers {
                f.items.push((ly.name.clone(), Some(ly.name.clone())));
            }
            f.current = if current.len() == 1 {
                let c = current.iter().next().cloned();
                f.items.iter().position(|(_, d)| *d == c).unwrap_or(0)
            } else {
                0
            };
        }
        self.layer = f;
    }

    /// A layer picked in the field: the selection moves onto it.
    pub fn layer_picked(&mut self, w: &mut MainWindow, index: usize) {
        let Some(Some(name)) = self.layer.items.get(index).map(|(_, d)| d.clone()) else {
            return;
        };
        let scene = &w.viewport.scene;
        let targets: Vec<EntityRef> = scene
            .selection
            .iter()
            .filter(|e| taggable(e) && layer_of(scene, e) != name)
            .copied()
            .collect();
        if targets.is_empty() {
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
        self.refresh(w);
    }

    fn solid_volume(&mut self, w: &MainWindow, g: GroupId) -> Option<f64> {
        let version = w.viewport.scene.version;
        if self.solid_cache.as_ref().is_none_or(|c| c.0 != version) {
            self.solid_cache = Some((version, HashMap::new()));
        }
        let cache = &mut self.solid_cache.as_mut()?.1;
        *cache.entry(g).or_insert_with(|| {
            let doc = &w.viewport.scene.doc;
            doc.find_group(g)
                .and_then(|grp| xtrude_model::solids::solid_volume(&doc.meshes, grp))
        })
    }

    /// What the selection is.
    pub fn describe(&mut self, w: &MainWindow, sel: &[EntityRef]) -> String {
        if sel.is_empty() {
            return tr("Nothing selected", &[]);
        }
        let scene = &w.viewport.scene;
        if let [e] = sel {
            match *e {
                EntityRef::Face(m, f) => {
                    let mesh = scene.doc.mesh(m);
                    if !mesh.has_face(f) {
                        return tr("Nothing selected", &[]);
                    }
                    let face = mesh.face(f);
                    let mat = match face.attrs.paint.color {
                        Some(c) => format!(
                            "color ({}, {}, {})",
                            (c[0] * 100.0).round() / 100.0,
                            (c[1] * 100.0).round() / 100.0,
                            (c[2] * 100.0).round() / 100.0
                        ),
                        None => "—".into(),
                    };
                    return format!(
                        "<b>{}</b><br>{}: {:.3} m²<br>{}: {}<br>{}: {}",
                        tr("Face", &[]),
                        tr("Area", &[]),
                        mesh.loop_area(&face.outer),
                        tr("Vertices", &[]),
                        face.outer.len(),
                        tr("Color", &[]),
                        mat
                    );
                }
                EntityRef::Edge(m, e) => {
                    let mesh = scene.doc.mesh(m);
                    if !mesh.has_edge(e) {
                        return tr("Nothing selected", &[]);
                    }
                    let (a, b) = mesh.edge_ab(e);
                    return format!(
                        "<b>{}</b><br>{}: {}",
                        tr("Edge", &[]),
                        tr("Length", &[]),
                        fmt_len((b - a).length())
                    );
                }
                EntityRef::Dimension(d) => {
                    if let Some(dim) = scene.doc.dimensions.iter().find(|x| x.id == d) {
                        return format!(
                            "<b>{}</b><br>{}: {}",
                            tr("Dimension", &[]),
                            tr("Measure", &[]),
                            fmt_len(dim.value(&scene.doc_view()))
                        );
                    }
                }
                EntityRef::Group(g) => {
                    let vol = self.solid_volume(w, g);
                    let scene = &w.viewport.scene;
                    let Some(grp) = scene.doc.find_group(g) else {
                        return tr("Nothing selected", &[]);
                    };
                    let faces = scene.doc.mesh(grp.mesh).face_ids().len();
                    let solid = vol.map_or(String::new(), |v| {
                        format!("<br>{}: {:.3} m³", tr("Volume", &[]), v)
                    });
                    if grp.is_component() {
                        let kin = scene
                            .doc
                            .groups
                            .iter()
                            .filter(|x| x.mesh == grp.mesh)
                            .count();
                        let title = if vol.is_some() {
                            tr("Solid Component", &[])
                        } else {
                            tr("Component", &[])
                        };
                        return format!(
                            "<b>{title}</b><br>{}: {faces}<br>{}: {kin}{solid}",
                            tr("Faces", &[]),
                            tr("In model", &[])
                        );
                    }
                    let title = if vol.is_some() {
                        tr("Solid Group", &[])
                    } else {
                        tr("Group", &[])
                    };
                    return format!("<b>{title}</b><br>{}: {faces}{solid}", tr("Faces", &[]));
                }
                _ => {}
            }
            return format!("<b>{}</b>", tr("1 entity", &[]));
        }
        let mut counts = [
            ("faces", 0usize),
            ("edges", 0),
            ("dimensions", 0),
            ("groups", 0),
        ];
        for e in sel {
            let i = match e {
                EntityRef::Face(..) => 0,
                EntityRef::Edge(..) => 1,
                EntityRef::Dimension(_) => 2,
                EntityRef::Group(_) => 3,
                _ => continue,
            };
            counts[i].1 += 1;
        }
        let parts: Vec<String> = counts
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(k, n)| format!("{n} {}", tr(k, &[])))
            .collect();
        format!("<b>{}</b><br>{}", tr("Selection", &[]), parts.join(", "))
    }
}
