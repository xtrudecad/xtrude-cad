// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The properties panel (`Properties`), under the model tree: facts about
//! the selection in the document's units. For one group or component: its
//! size along the model axes and where its box starts, its area, whether
//! it is a closed solid (and why not, with a way to show the edges to
//! blame), its volume, print material and mass; for several groups, their
//! combined size, volume and mass. The name of one group or component and
//! the selection's layer are edited here, and the print material is set.

use std::collections::{BTreeSet, HashMap};

use glam::DVec3;
use xtrude_model::commands::{layer_of, AssignLayerCommand, RenameGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{iter_placements, transformed_mesh};
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::print_material::{group_mass, print_material_from, MaterialFrom};
use xtrude_model::scene::parents_placement;
use xtrude_model::units::{fmt_area, fmt_len, fmt_mass, fmt_triple, fmt_volume};

use crate::window::model_info::color_label;
use crate::window::solid_inspector::{inspect_mesh, problem_text, SolidReport};
use crate::window::CtxItem;
use crate::window::MainWindow;

/// What the panel measures of one group, once per document version.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupFacts {
    /// The box along the model axes, `(lo, hi)`; `None` without vertices.
    pub bounds: Option<(DVec3, DVec3)>,
    /// The surface area of its faces and its nested groups', m².
    pub area: f64,
    /// The bodies (the group and its nested groups that hold faces).
    pub bodies: usize,
    /// The edges to blame, summed over the bodies (the Solid Inspector's
    /// count); empty when every body is closed.
    pub problems: SolidReport,
    /// The first body that is not closed, to show.
    pub problem_body: Option<GroupId>,
    /// The volume when every body is closed, m³.
    pub volume: Option<f64>,
}

impl GroupFacts {
    /// Every body closed: a solid.
    pub fn closed(&self) -> bool {
        self.bodies > 0 && self.problem_body.is_none()
    }

    /// Measure group `g`, its parents' placements applied.
    pub fn of(w: &MainWindow, g: GroupId) -> GroupFacts {
        let doc = &w.viewport.scene.doc;
        let Some(path) = doc.group_path(g) else {
            return GroupFacts::default();
        };
        let mut facts = GroupFacts::default();
        let pts = doc.group_world_points(g);
        facts.bounds = pts.first().map(|&p0| {
            pts.iter()
                .fold((p0, p0), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
        });
        let mut volume = 0.0;
        for (body, m) in iter_placements(path[path.len() - 1], parents_placement(&path)) {
            let mesh = doc.mesh(body.mesh);
            if mesh.faces.is_empty() {
                continue;
            }
            facts.bodies += 1;
            let placed = m.map(|m| transformed_mesh(mesh, &m));
            let shown = placed.as_ref().unwrap_or(mesh);
            facts.area += shown
                .faces
                .iter()
                .filter(|(_, f)| !f.interior)
                .map(|(id, _)| shown.face_area(id))
                .sum::<f64>();
            let report = inspect_mesh(mesh);
            match report.volume {
                Some(v) if report.watertight => {
                    volume += v * m.map_or(1.0, |m| m.determinant().abs());
                }
                _ => {
                    facts.problem_body.get_or_insert(body.id);
                    facts.problems.open.extend(report.open);
                    facts.problems.stray.extend(report.stray);
                    facts.problems.over.extend(report.over);
                }
            }
        }
        facts.volume = facts.closed().then_some(volume);
        facts
    }

    /// The size, W × D × H along the model axes.
    pub fn size_text(&self) -> Option<String> {
        self.bounds.map(|(lo, hi)| {
            let d = hi - lo;
            fmt_triple(d.x, d.y, d.z, false)
        })
    }

    /// The solid check's line.
    pub fn check_text(&self) -> String {
        if self.bodies == 0 {
            tr("No faces: not a solid", &[])
        } else if self.closed() {
            format!("✔ {}", tr("Closed solid", &[]))
        } else {
            format!(
                "✖ {}",
                tr(
                    "Not closed: {problems}",
                    &[("problems", &problem_text(&self.problems))]
                )
            )
        }
    }
}

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
    /// The one group selected is not a closed solid: Show… opens it on
    /// the edges to blame.
    pub can_show_problems: bool,
    /// The print material entries for the groups selected (the context
    /// menu's: the built-ins, Custom…, Use default), `None` without
    /// groups; each one is a window action.
    pub material_menu: Option<Vec<CtxItem>>,
    facts_cache: Option<(u64, HashMap<GroupId, GroupFacts>)>,
    mass_cache: Option<(u64, HashMap<GroupId, Option<f64>>)>,
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
        self.can_show_problems = match self.named {
            Some(g) => self.facts(w, g).problem_body.is_some(),
            None => false,
        };
        self.material_menu = match w.print_material_submenu() {
            Some(CtxItem::Menu(_, items)) => Some(items),
            _ => None,
        };
    }

    /// Show…: open the first body of the selected group that is not
    /// closed, and the Solid Inspector on it, its edges to blame selected.
    pub fn show_problems(&mut self, w: &mut MainWindow) {
        let Some(body) = self.named.and_then(|g| self.facts(w, g).problem_body) else {
            return;
        };
        super::edit_group(w, body);
        w.trigger("solid_inspector");
        if let Some(mut dlg) = w.solid_inspector.take() {
            dlg.refresh(&w.viewport.scene);
            // the open group's row is the first
            dlg.current = Some(0);
            dlg.highlight(&mut w.viewport);
            w.solid_inspector = Some(dlg);
        }
        w.pump_events();
        self.refresh(w);
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

    /// What the panel measures of group `g`, cached per document version.
    pub fn facts(&mut self, w: &MainWindow, g: GroupId) -> GroupFacts {
        let version = w.viewport.scene.version;
        if self.facts_cache.as_ref().is_none_or(|c| c.0 != version) {
            self.facts_cache = Some((version, HashMap::new()));
        }
        match self.facts_cache.as_mut() {
            Some((_, cache)) => cache
                .entry(g)
                .or_insert_with(|| GroupFacts::of(w, g))
                .clone(),
            None => GroupFacts::of(w, g),
        }
    }

    fn group_mass(&mut self, w: &MainWindow, g: GroupId) -> Option<f64> {
        let version = w.viewport.scene.version;
        if self.mass_cache.as_ref().is_none_or(|c| c.0 != version) {
            self.mass_cache = Some((version, HashMap::new()));
        }
        let cache = &mut self.mass_cache.as_mut()?.1;
        *cache
            .entry(g)
            .or_insert_with(|| group_mass(&w.viewport.scene.doc, g))
    }

    /// A group's print material and mass, as two lines.
    fn material_lines(&mut self, w: &MainWindow, g: GroupId) -> String {
        let mass = self.group_mass(w, g);
        let doc = &w.viewport.scene.doc;
        let (material, from) = print_material_from(doc, g);
        let material = match from {
            MaterialFrom::Own => material.name.clone(),
            MaterialFrom::Part(p) => tr(
                "{name} (from {part})",
                &[
                    ("name", &material.name),
                    (
                        "part",
                        &doc.find_group(p)
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                    ),
                ],
            ),
            MaterialFrom::Document => tr("{name} (document)", &[("name", &material.name)]),
        };
        let mass = match mass {
            Some(kg) => format!("{}: {}", tr("Mass (100 % infill)", &[]), fmt_mass(kg)),
            None => tr("Not a closed solid: no mass", &[]),
        };
        format!("<br>{}: {material}<br>{mass}", tr("Print material", &[]))
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
                    let paint = &face.attrs.paint;
                    let mat = match paint.color {
                        Some(c) => color_label(c, paint.mat.as_deref()),
                        None => "—".into(),
                    };
                    return format!(
                        "<b>{}</b><br>{}: {}<br>{}: {}<br>{}: {}",
                        tr("Face", &[]),
                        tr("Area", &[]),
                        fmt_area(mesh.loop_area(&face.outer)),
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
                    let facts = self.facts(w, g);
                    let vol = facts.volume;
                    let printed = self.material_lines(w, g);
                    let scene = &w.viewport.scene;
                    let Some(grp) = scene.doc.find_group(g) else {
                        return tr("Nothing selected", &[]);
                    };
                    let faces = scene.doc.mesh(grp.mesh).face_ids().len();
                    let mut solid = String::new();
                    if let Some((size, (lo, _))) = facts.size_text().zip(facts.bounds) {
                        solid += &format!(
                            "<br>{}: {size}<br>{}: {}, {}, {}",
                            tr("Size", &[]),
                            tr("Origin", &[]),
                            fmt_len(lo.x),
                            fmt_len(lo.y),
                            fmt_len(lo.z)
                        );
                    }
                    solid += &format!("<br>{}: {}", tr("Area", &[]), fmt_area(facts.area));
                    if let Some(v) = vol {
                        solid += &format!("<br>{}: {}", tr("Volume", &[]), fmt_volume(v));
                    }
                    solid += &format!("<br>{}", facts.check_text());
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
                            "<b>{title}</b><br>{}: {faces}<br>{}: {kin}{solid}{printed}",
                            tr("Faces", &[]),
                            tr("In model", &[])
                        );
                    }
                    let title = if vol.is_some() {
                        tr("Solid Group", &[])
                    } else {
                        tr("Group", &[])
                    };
                    return format!(
                        "<b>{title}</b><br>{}: {faces}{solid}{printed}",
                        tr("Faces", &[])
                    );
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
        let mut text = format!("<b>{}</b><br>{}", tr("Selection", &[]), parts.join(", "));
        let groups: Vec<GroupId> = sel
            .iter()
            .filter_map(|e| match e {
                EntityRef::Group(g) => Some(*g),
                _ => None,
            })
            .collect();
        if groups.len() > 1 {
            text += &self.totals(w, &groups);
        }
        text
    }

    /// Several groups: their combined size along the model axes, and their
    /// volume and mass when every one has them.
    fn totals(&mut self, w: &MainWindow, groups: &[GroupId]) -> String {
        let mut out = String::new();
        let facts: Vec<GroupFacts> = groups.iter().map(|&g| self.facts(w, g)).collect();
        let bounds = facts
            .iter()
            .filter_map(|f| f.bounds)
            .reduce(|(alo, ahi), (blo, bhi)| (alo.min(blo), ahi.max(bhi)));
        if let Some((lo, hi)) = bounds {
            let d = hi - lo;
            out += &format!(
                "<br>{}: {}",
                tr("Combined size", &[]),
                fmt_triple(d.x, d.y, d.z, false)
            );
        }
        let volume: Option<f64> = facts.iter().map(|f| f.volume).sum();
        if let Some(v) = volume {
            out += &format!("<br>{}: {}", tr("Total volume", &[]), fmt_volume(v));
        }
        let mass: Option<f64> = groups.iter().map(|&g| self.group_mass(w, g)).sum();
        match mass {
            Some(kg) => {
                out += &format!(
                    "<br>{}: {}",
                    tr("Total mass (100 % infill)", &[]),
                    fmt_mass(kg)
                )
            }
            None => out += &format!("<br>{}", tr("Not all closed solids: no total mass", &[])),
        }
        out
    }
}
