// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Edit and Camera menus' one-shot commands. Every path ANSWERS: a refusal
//! says why in the status bar.

use glam::DVec3;
use xtrude_core::orient_faces::faces_to_flip;
use xtrude_core::Axis;
use xtrude_model::commands::{
    AssignLayerCommand, DeleteDimensionsCommand, DeleteGroupCommand, DeleteGuidesCommand,
    DeleteSectionPlanesCommand, DeleteTextLabelsCommand, EraseSelectionCommand,
    ExplodeGroupCommand, FlipFacesCommand, GroupToComponentCommand, HealOverlapsCommand,
    HideCommand, MakeComponentOfCommand, MakeGroupCommand, MakeIndependentCommand,
    MakeNestedGroupCommand, MergeGroupsCommand, RebuildPlanarFacesCommand,
    RebuildPlaneFacesCommand, SimplifyMeshCommand, SplitIntoPiecesCommand,
};
use xtrude_model::edits::{build_add_edges, divide_edges};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::{Command, CompoundCommand, FnCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::intersect::{segments_for, IntersectMode};
use xtrude_model::pieces::split_into_pieces;
use xtrude_model::scene::{Scene, SelectMode};
use xtrude_model::settings::{self, keys};
use xtrude_model::style::{style_by_name, Style};

use super::MainWindow;
use crate::tools::flip::FlipTool;
use crate::tools::paste::PasteTool;
use crate::tools::{paint_sample_armed, set_paint_sample_armed};

/// The selection split by kind: loose faces and edges of the open context,
/// groups.
pub(super) struct Sel {
    pub faces: Vec<xtrude_core::FaceId>,
    pub edges: Vec<xtrude_core::EdgeId>,
    pub groups: Vec<GroupId>,
}

pub(super) fn split_selection(scene: &Scene) -> Sel {
    let mid = scene.active_mesh_id();
    let mut s = Sel {
        faces: Vec::new(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    for e in scene.selection.iter() {
        match *e {
            EntityRef::Face(m, f) if m == mid => s.faces.push(f),
            EntityRef::Edge(m, x) if m == mid => s.edges.push(x),
            EntityRef::Group(g) => s.groups.push(g),
            _ => {}
        }
    }
    s
}

/// The name Make Component suggests: "Component #n", n one more than the
/// components at every depth, or the next number no group uses yet.
fn default_component_name(scene: &Scene) -> String {
    let mut all = Vec::new();
    for g in &scene.doc.groups {
        g.walk(&mut all);
    }
    let mut n = all.iter().filter(|g| scene.is_component(g)).count() + 1;
    loop {
        let name = tr("Component #{n}", &[("n", &n)]);
        if !all.iter().any(|g| g.name == name) {
            return name;
        }
        n += 1;
    }
}

/// Whether an object, face or edge is hidden.
pub(super) fn is_hidden(scene: &Scene, e: &EntityRef) -> bool {
    match *e {
        EntityRef::Edge(m, x) => {
            let mesh = scene.doc.mesh(m);
            mesh.has_edge(x) && mesh.edge(x).hidden
        }
        EntityRef::Face(m, f) => {
            let mesh = scene.doc.mesh(m);
            mesh.has_face(f) && mesh.face(f).attrs.hidden
        }
        EntityRef::Group(g) => scene.doc.find_group(g).is_some_and(|g| g.hidden),
        _ => false,
    }
}

impl MainWindow {
    // ---- groups -------------------------------------------------------------------------------
    pub(super) fn on_make_group(&mut self) {
        let vp = &mut self.viewport;
        let s = split_selection(&vp.scene);
        if s.groups.len() == 1 && s.faces.is_empty() && s.edges.is_empty() {
            // One group alone: a Russian doll with nothing inside.
            vp.flash_status(
                &tr(
                    "That is already a group — select something else with it to group them together",
                    &[],
                ),
                4000,
            );
            return;
        }
        if !s.groups.is_empty() {
            let loose = !(s.faces.is_empty() && s.edges.is_empty());
            let n = s.groups.len();
            let cmd =
                MakeNestedGroupCommand::new(vp.scene.mesh(), &s.faces, &s.edges, s.groups, None);
            vp.run(cmd);
            let extra = if loose {
                tr(" and the loose geometry", &[])
            } else {
                String::new()
            };
            vp.flash_status(
                &tr(
                    "Grouped {n} group(s){extra} — double-click to go inside",
                    &[("n", &n), ("extra", &extra)],
                ),
                4000,
            );
            vp.update();
            return;
        }
        if s.faces.is_empty() && s.edges.is_empty() {
            vp.flash_status(&tr("Select the geometry to group first", &[]), 4000);
            return;
        }
        let cmd = MakeGroupCommand::new(vp.scene.mesh(), &s.faces, &s.edges, false, None);
        vp.run(cmd);
        vp.update();
    }

    pub(super) fn on_make_component(&mut self) {
        let s = split_selection(&self.viewport.scene);
        let default_name = default_component_name(&self.viewport.scene);
        let created = tr("Component created — copies will share its definition", &[]);
        let loose = !(s.faces.is_empty() && s.edges.is_empty());
        if s.groups.len() > 1 || (!s.groups.is_empty() && loose) {
            // ONE component holding them, each still a group inside.
            let Some(name) = self.dialogs.get_text(
                &tr("Make Component", &[]),
                &tr("Component name:", &[]),
                &default_name,
                false,
            ) else {
                return;
            };
            let name = Some(name.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or(default_name);
            let nest = MakeNestedGroupCommand::new(
                self.viewport.scene.mesh(),
                &s.faces,
                &s.edges,
                s.groups,
                Some(&name),
            );
            self.viewport.run(MakeComponentOfCommand { nest });
            self.viewport.update();
            self.show_message(&created, 4000);
            return;
        }
        if let Some(&gid) = s.groups.first() {
            // One group converts in place — its mesh becomes the shared
            // definition, free.
            let Some(g) = self.viewport.scene.doc.find_group(gid) else {
                return;
            };
            if g.is_component() {
                self.viewport
                    .flash_status(&tr("It is a component already", &[]), 2500);
                return;
            }
            let current = if g.name.is_empty() {
                tr("Component", &[])
            } else {
                g.name.clone()
            };
            let Some(name) = self.dialogs.get_text(
                &tr("Make Component", &[]),
                &tr("Component name:", &[]),
                &current,
                false,
            ) else {
                return;
            };
            let name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
            self.viewport
                .run(GroupToComponentCommand { group: gid, name });
            self.viewport.update();
            self.show_message(&created, 4000);
            return;
        }
        if !loose {
            self.viewport.flash_status(
                &tr("Select the geometry for the component first", &[]),
                2500,
            );
            return;
        }
        let Some(name) = self.dialogs.get_text(
            &tr("Make Component", &[]),
            &tr("Component name:", &[]),
            &default_name,
            false,
        ) else {
            return;
        };
        let name = Some(name.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or(default_name);
        let cmd = MakeGroupCommand::new(
            self.viewport.scene.mesh(),
            &s.faces,
            &s.edges,
            true,
            Some(&name),
        );
        self.viewport.run(cmd);
        self.viewport.update();
        self.show_message(&created, 4000);
    }

    pub(super) fn on_make_independent(&mut self) {
        let groups: Vec<GroupId> = split_selection(&self.viewport.scene)
            .groups
            .into_iter()
            .filter(|g| {
                self.viewport
                    .scene
                    .doc
                    .find_group(*g)
                    .is_some_and(|g| g.is_component())
            })
            .collect();
        for g in groups {
            self.viewport.run(MakeIndependentCommand { group: g });
        }
        self.viewport.update();
    }

    pub(super) fn on_merge_groups(&mut self) {
        let groups = split_selection(&self.viewport.scene).groups;
        if groups.len() < 2 {
            return;
        }
        let n = groups.len();
        self.viewport.end_group_edit();
        self.viewport.run(MergeGroupsCommand {
            groups,
            merged: None,
        });
        self.viewport.update();
        self.show_message(&tr("{n} groups merged into one", &[("n", &n)]), 3000);
    }

    /// Right-click ▸ Change Axes: three clicks give the group new axes.
    pub(super) fn on_change_axes(&mut self, group: GroupId) {
        let tool = crate::tools::change_axes::ChangeAxesTool::for_target(group);
        self.activate_transient(Box::new(tool));
        self.viewport
            .flash_status(&tr("Click the new origin", &[]), 4000);
    }

    pub(super) fn on_add_crossing_edges(&mut self, mode: IntersectMode) {
        if self.viewport.scene.selection.is_empty() {
            self.viewport
                .flash_status(&tr("Select faces or groups first", &[]), 2500);
            return;
        }
        let segs = segments_for(&self.viewport.scene, mode);
        if segs.is_empty() {
            self.viewport
                .flash_status(&tr("No faces cross the selection", &[]), 2500);
            return;
        }
        let n = segs.len();
        let cmd = build_add_edges(&self.viewport.scene, &segs, true, vec![], None);
        self.viewport.execute_boxed(cmd);
        self.viewport
            .flash_status(&tr("{n} intersection edges added", &[("n", &n)]), 3000);
    }

    pub(super) fn on_split_into_pieces(&mut self) {
        if self.viewport.scene.edit_group().is_some() {
            self.viewport.flash_status(
                &tr("Leave the group first (Esc) to split it into pieces", &[]),
                2500,
            );
            return;
        }
        let groups = split_selection(&self.viewport.scene).groups;
        if groups.len() != 1 {
            self.viewport.flash_status(
                &tr("Select one group or component to split into pieces", &[]),
                2500,
            );
            return;
        }
        let Some(g) = self.viewport.scene.doc.find_group(groups[0]).cloned() else {
            return;
        };
        let pieces = split_into_pieces(&mut self.viewport.scene.doc.meshes, &g);
        if pieces.is_empty() {
            self.viewport.flash_status(
                &tr("Nothing to split: it is already in its pieces", &[]),
                2500,
            );
            return;
        }
        let n = pieces.len();
        self.viewport.run(SplitIntoPiecesCommand {
            group: g.id,
            pieces,
        });
        self.viewport.flash_status(
            &tr(
                "Split into {n} pieces — Explode Group sets them free",
                &[("n", &n)],
            ),
            5000,
        );
        self.viewport.update();
    }

    pub(super) fn on_explode_group(&mut self) {
        let groups = split_selection(&self.viewport.scene).groups;
        let any = !groups.is_empty();
        for g in groups {
            self.viewport.run(ExplodeGroupCommand { group: g });
        }
        if any {
            self.viewport.update();
        }
    }

    pub(super) fn on_simplify_mesh(&mut self) {
        if self.viewport.scene.mesh().faces.is_empty() {
            let msg = if self.viewport.scene.doc.groups.is_empty() {
                "There is no mesh to simplify"
            } else {
                "Explode the imported group first, then simplify the mesh"
            };
            self.viewport.flash_status(&tr(msg, &[]), 2500);
            return;
        }
        let Some(angle) = self.dialogs.get_number(
            &tr("Simplify Mesh", &[]),
            &tr(
                "Maximum facet angle in degrees (0 = coplanar only; higher values may change rounded geometry):",
                &[],
            ),
            1.0,
            0.0,
            5.0,
            2,
        ) else {
            return;
        };
        let mut cmd = SimplifyMeshCommand {
            max_angle_degrees: angle,
            faces_removed: 0,
        };
        if !self.viewport.execute(&mut cmd) {
            let err = self.viewport.history.last_error.clone().unwrap_or_default();
            self.dialogs.warn(&tr("Simplify Mesh failed", &[]), &err);
            return;
        }
        if cmd.faces_removed > 0 {
            self.viewport.flash_status(
                &tr(
                    "Simplified mesh: removed {count} faces",
                    &[("count", &cmd.faces_removed)],
                ),
                5000,
            );
        } else {
            self.viewport
                .flash_status(&tr("No faces could be simplified", &[]), 2500);
        }
        self.viewport.update();
    }

    pub(super) fn on_delete_guides(&mut self) {
        let ids: Vec<_> = self
            .viewport
            .scene
            .doc
            .guides
            .iter()
            .map(|g| g.id)
            .collect();
        if !ids.is_empty() {
            self.viewport.run(DeleteGuidesCommand { ids });
            self.viewport.update();
        }
    }

    /// Delete the current selection (any kind) as one step — the Select
    /// tool's Delete, from the context menu whatever the tool.
    pub(super) fn on_delete_selection(&mut self) {
        let scene = &self.viewport.scene;
        if scene.selection.is_empty() {
            return;
        }
        let s = split_selection(scene);
        let pick = |f: fn(&EntityRef) -> Option<xtrude_model::ids::ObjId>| {
            scene.selection.iter().filter_map(f).collect::<Vec<_>>()
        };
        let dims = pick(|e| match e {
            EntityRef::Dimension(d) => Some(*d),
            _ => None,
        });
        let labels = pick(|e| match e {
            EntityRef::TextLabel(d) => Some(*d),
            _ => None,
        });
        let guides = pick(|e| match e {
            EntityRef::Guide(d) => Some(*d),
            _ => None,
        });
        let planes = pick(|e| match e {
            EntityRef::SectionPlane(d) => Some(*d),
            _ => None,
        });
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        if !s.edges.is_empty() || !s.faces.is_empty() {
            cmds.push(Box::new(EraseSelectionCommand::from_entities(
                scene.mesh(),
                &s.edges,
                &s.faces,
            )));
        }
        for g in s.groups {
            cmds.push(Box::new(DeleteGroupCommand { group: g }));
        }
        if !guides.is_empty() {
            cmds.push(Box::new(DeleteGuidesCommand { ids: guides }));
        }
        if !planes.is_empty() {
            cmds.push(Box::new(DeleteSectionPlanesCommand { ids: planes }));
        }
        if !dims.is_empty() {
            cmds.push(Box::new(DeleteDimensionsCommand { ids: dims }));
        }
        if !labels.is_empty() {
            cmds.push(Box::new(DeleteTextLabelsCommand { ids: labels }));
        }
        self.run_commands(cmds);
        self.viewport.update();
    }

    fn run_commands(&mut self, mut cmds: Vec<Box<dyn Command>>) {
        match cmds.len() {
            0 => {}
            1 => {
                let c = cmds.pop().expect("one command");
                self.viewport.execute_boxed(c);
            }
            _ => {
                self.viewport.execute(&mut CompoundCommand::new(cmds));
            }
        }
    }

    // ---- hide / unhide -------------------------------------------------------------------------
    /// The objects, faces and edges Hide acts on (the ones not hidden yet).
    pub(super) fn hideable(&self) -> Vec<EntityRef> {
        let scene = &self.viewport.scene;
        scene
            .selection
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EntityRef::Edge(..) | EntityRef::Face(..) | EntityRef::Group(_)
                )
            })
            .filter(|e| !is_hidden(scene, e))
            .copied()
            .collect()
    }

    pub(super) fn on_hide(&mut self) {
        let targets = self.hideable();
        if targets.is_empty() {
            self.show_message(&tr("Select an object, faces or edges first.", &[]), 3000);
            return;
        }
        let n_obj = targets.iter().filter(|e| e.is_group()).count();
        let n_face = targets.iter().filter(|e| e.is_face()).count();
        let n_edge = targets.len() - n_obj - n_face;
        self.viewport.run(HideCommand {
            entities: targets,
            hidden: true,
        });
        self.viewport.update();
        self.show_message(
            &tr(
                "Hid {objects} object(s), {faces} face(s) and {edges} edge(s) — Edit > Unhide brings them back",
                &[("objects", &n_obj), ("faces", &n_face), ("edges", &n_edge)],
            ),
            4000,
        );
    }

    pub(super) fn on_unhide_selected(&mut self) {
        let scene = &self.viewport.scene;
        let targets: Vec<EntityRef> = scene
            .selection
            .iter()
            .filter(|e| is_hidden(scene, e))
            .copied()
            .collect();
        if targets.is_empty() {
            self.show_message(
                &tr(
                    "Nothing hidden in the selection — turn on Camera > Hidden Groups or Hidden Edges and Faces to select hidden things.",
                    &[],
                ),
                4000,
            );
            return;
        }
        let n = targets.len();
        self.viewport.run(HideCommand {
            entities: targets,
            hidden: false,
        });
        self.viewport.update();
        self.show_message(&tr("Unhid {n} entities.", &[("n", &n)]), 3000);
    }

    /// The most recent Hide whose entities are still hidden comes back.
    pub(super) fn on_unhide_last(&mut self) {
        let scene = &self.viewport.scene;
        let still = self.viewport.history.hide_steps().find_map(|step| {
            let still: Vec<EntityRef> = step
                .iter()
                .filter(|e| is_hidden(scene, e))
                .copied()
                .collect();
            (!still.is_empty()).then_some(still)
        });
        match still {
            Some(still) => {
                let n = still.len();
                self.viewport.run(HideCommand {
                    entities: still,
                    hidden: false,
                });
                self.viewport.update();
                self.show_message(&tr("Unhid {n} entities.", &[("n", &n)]), 3000);
            }
            None => self.show_message(&tr("Nothing to unhide.", &[]), 3000),
        }
    }

    /// Every hidden object (nested ones too) plus the hidden faces and edges
    /// of the mesh being edited.
    pub(super) fn hidden_everywhere(&self) -> Vec<EntityRef> {
        let scene = &self.viewport.scene;
        let mut out: Vec<EntityRef> = scene
            .doc
            .all_groups()
            .into_iter()
            .filter(|g| g.hidden)
            .map(|g| EntityRef::Group(g.id))
            .collect();
        let mid = scene.active_mesh_id();
        let m = scene.mesh();
        out.extend(
            m.face_ids()
                .into_iter()
                .filter(|&f| m.face(f).attrs.hidden)
                .map(|f| EntityRef::Face(mid, f)),
        );
        out.extend(
            m.edge_ids()
                .into_iter()
                .filter(|&e| m.edge(e).hidden)
                .map(|e| EntityRef::Edge(mid, e)),
        );
        out
    }

    pub(super) fn on_unhide_all(&mut self) {
        let targets = self.hidden_everywhere();
        if targets.is_empty() {
            self.show_message(&tr("Nothing is hidden.", &[]), 3000);
            return;
        }
        let n = targets.len();
        self.viewport.run(HideCommand {
            entities: targets,
            hidden: false,
        });
        self.viewport.update();
        self.show_message(&tr("Unhid {n} entities.", &[("n", &n)]), 3000);
    }

    // ---- faces ---------------------------------------------------------------------------------
    pub(super) fn on_divide(&mut self) {
        let edges = split_selection(&self.viewport.scene).edges;
        if edges.is_empty() {
            return;
        }
        let Some(n) = self.dialogs.get_number(
            &tr("Divide", &[]),
            &tr("Number of segments:", &[]),
            2.0,
            2.0,
            500.0,
            0,
        ) else {
            return;
        };
        let n = n.round().max(2.0) as usize;
        self.viewport
            .run(FnCommand::new("SnapshotMutation", move |s: &mut Scene| {
                divide_edges(s.mesh_mut(), &edges, n);
                Ok(())
            }));
        self.viewport.scene.selection.clear();
        self.viewport.scene.version += 1;
        self.viewport.update();
        self.show_message(&tr("Divided into {n} segments", &[("n", &n)]), 3000);
    }

    /// Right-click ▸ Flip Along ▸ Red / Green / Blue.
    pub(super) fn on_flip_along(&mut self, axis: Axis) {
        FlipTool::flip(&mut self.viewport, axis, false);
    }

    fn selected_faces(&self) -> Vec<(MeshId, xtrude_core::FaceId)> {
        let mid = self.viewport.scene.active_mesh_id();
        split_selection(&self.viewport.scene)
            .faces
            .into_iter()
            .map(|f| (mid, f))
            .collect()
    }

    pub(super) fn on_reverse_faces(&mut self) {
        let faces = self.selected_faces();
        if faces.is_empty() {
            self.show_message(&tr("Select one or more faces first.", &[]), 3000);
            return;
        }
        let n = faces.len();
        self.viewport.run(FlipFacesCommand { faces });
        self.viewport.update();
        self.show_message(&tr("Reversed {n} face(s).", &[("n", &n)]), 3000);
    }

    pub(super) fn on_orient_faces(&mut self) {
        let faces = self.selected_faces();
        if faces.len() != 1 {
            self.show_message(
                &tr("Select the one face the others should match.", &[]),
                3000,
            );
            return;
        }
        let (mid, seed) = faces[0];
        let flip = faces_to_flip(self.viewport.scene.doc.mesh(mid), seed);
        let n = flip.len();
        if n > 0 {
            self.viewport.run(FlipFacesCommand {
                faces: flip.into_iter().map(|f| (mid, f)).collect(),
            });
            self.viewport.update();
            self.show_message(
                &tr("Oriented {n} face(s) like the selected one.", &[("n", &n)]),
                3000,
            );
        } else {
            self.show_message(&tr("The connected faces already match.", &[]), 3000);
        }
    }

    pub(super) fn on_heal_overlaps(&mut self) {
        let mut cmd = HealOverlapsCommand::default();
        self.viewport.execute(&mut cmd);
        self.viewport.update();
        let msg = if cmd.healed > 0 {
            tr("Healed {n} overlapping face(s).", &[("n", &cmd.healed)])
        } else {
            tr("No overlapping faces found.", &[])
        };
        self.show_message(&msg, 3000);
    }

    /// `(origin, unit normal)` of the plane every face lies on, or `None`.
    fn common_plane(&self, faces: &[(MeshId, xtrude_core::FaceId)]) -> Option<(DVec3, DVec3)> {
        let (mid, f0) = *faces.first()?;
        let m = self.viewport.scene.doc.mesh(mid);
        let n = m.face_normal(f0);
        if n.length() < 1e-9 {
            return None;
        }
        let n = n.normalize();
        let o = m.face_positions(f0)[0];
        let tol = 1e-4;
        for &(mid, f) in faces {
            let m = self.viewport.scene.doc.mesh(mid);
            let mut pts = m.face_positions(f);
            pts.extend(m.face_hole_positions(f).into_iter().flatten());
            if pts.iter().any(|p| (*p - o).dot(n).abs() > tol) {
                return None;
            }
        }
        Some((o, n))
    }

    pub(super) fn on_rebuild_planar(&mut self) {
        let picked = self.selected_faces();
        if !picked.is_empty() {
            let Some((o, n)) = self.common_plane(&picked) else {
                self.show_message(&tr("The selected faces must lie on one plane.", &[]), 3000);
                return;
            };
            let mut cmd = RebuildPlaneFacesCommand::new(o, n);
            self.viewport.execute(&mut cmd);
            self.viewport.update();
            self.show_message(
                &tr(
                    "Rebuilt {n} face(s) on the selected plane.",
                    &[("n", &cmd.rebuilt)],
                ),
                3000,
            );
            return;
        }
        let mut cmd = RebuildPlanarFacesCommand::new();
        self.viewport.execute(&mut cmd);
        self.viewport.update();
        let msg = if !cmd.flat {
            tr(
                "Rebuild Faces only works on a flat (single-plane) drawing.",
                &[],
            )
        } else {
            tr(
                "Rebuilt {n} face(s) from the edge graph.",
                &[("n", &cmd.rebuilt)],
            )
        };
        self.show_message(&msg, 3000);
    }

    // ---- layers ---------------------------------------------------------------------------------
    /// Move the selection's taggable entities onto `name` (created if new).
    pub(super) fn assign_layer(&mut self, name: &str) {
        let scene = &self.viewport.scene;
        let targets: Vec<EntityRef> = scene
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
            .filter(|e| xtrude_model::commands::layer_of(scene, e) != name)
            .copied()
            .collect();
        if targets.is_empty() {
            return;
        }
        let n = targets.len();
        self.viewport.run(AssignLayerCommand {
            entities: targets,
            layer: name.to_string(),
        });
        self.viewport.update();
        self.show_message(
            &tr(
                "{n} entities moved to '{layer}'",
                &[("n", &n), ("layer", &name)],
            ),
            2500,
        );
    }

    pub(super) fn assign_new_layer(&mut self) {
        let base = tr("Layer", &[]);
        let mut n = 1;
        while self
            .viewport
            .scene
            .doc
            .layer(&format!("{base} {n}"))
            .is_some()
        {
            n += 1;
        }
        let Some(name) = self.dialogs.get_text(
            &tr("New layer", &[]),
            &tr("Layer name:", &[]),
            &format!("{base} {n}"),
            false,
        ) else {
            return;
        };
        let name = name.trim().to_string();
        if !name.is_empty() {
            self.assign_layer(&name);
        }
    }

    // ---- clipboard ------------------------------------------------------------------------------
    /// A copy made in ANOTHER Xtrude CAD window wins over this window's
    /// older one, as a system clipboard does.
    fn sync_foreign_clipboard(&mut self) {
        if let Some(other) = super::clipboard::foreign(self.system_clipboard.as_ref()) {
            self.viewport.clipboard = Some(other);
        }
    }

    pub(super) fn on_paste_in_place(&mut self) {
        self.sync_foreign_clipboard();
        if PasteTool::in_place(&mut self.viewport) {
            self.show_message(&tr("Pasted in place.", &[]), 3000);
        }
    }

    pub(super) fn on_paste(&mut self) {
        self.sync_foreign_clipboard();
        if self.viewport.clipboard.is_none() {
            return;
        }
        self.activate_transient(Box::new(PasteTool::new()));
    }

    // ---- selection -------------------------------------------------------------------------------
    /// Select every entity — edges, faces, groups (the open group's
    /// children inside it) and dimensions.
    pub(super) fn on_select_all(&mut self) {
        let scene = &mut self.viewport.scene;
        let mid = scene.active_mesh_id();
        let m = scene.mesh();
        let mut all: Vec<EntityRef> = m
            .edge_ids()
            .into_iter()
            .map(|e| EntityRef::Edge(mid, e))
            .collect();
        all.extend(m.face_ids().into_iter().map(|f| EntityRef::Face(mid, f)));
        match scene.edit_group() {
            None => all.extend(scene.doc.groups.iter().map(|g| EntityRef::Group(g.id))),
            Some(ctx) => {
                if let Some(c) = scene.doc.find_group(ctx) {
                    all.extend(c.children.iter().map(|g| EntityRef::Group(g.id)));
                }
            }
        }
        all.extend(
            scene
                .doc
                .dimensions
                .iter()
                .map(|d| EntityRef::Dimension(d.id)),
        );
        scene.selection.clear();
        scene.selection.extend(all);
        scene.version += 1;
        let n = scene.selection.len();
        self.viewport.notify_scene_changed();
        self.show_message(
            &tr("Selected everything ({n} entities)", &[("n", &n)]),
            2500,
        );
    }

    pub(super) fn on_invert_selection(&mut self) {
        let n = self.viewport.scene.invert_selection();
        self.viewport.notify_scene_changed();
        self.show_message(&tr("Selection inverted ({n} entities)", &[("n", &n)]), 2500);
    }

    /// Grow the selection by what it touches or shares.
    pub(super) fn grow_selection(&mut self, how: &str) {
        let scene = &self.viewport.scene;
        let mid = scene.active_mesh_id();
        let ents: Vec<EntityRef> = scene.selection.iter().copied().collect();
        let found = match how {
            "all_connected" => xtrude_model::select_ops::all_connected(scene.mesh(), mid, &ents),
            "bounding_edges" => xtrude_model::select_ops::bounding_edges(scene.mesh(), mid, &ents),
            "same_color" => xtrude_model::select_ops::same_material(scene, &ents),
            _ => xtrude_model::select_ops::same_layer(scene, &ents),
        };
        self.viewport.scene.select(found, SelectMode::Replace);
        self.viewport.notify_scene_changed();
        self.viewport.update();
        let n = self.viewport.scene.selection.len();
        self.show_message(&tr("{n} entities selected", &[("n", &n)]), 2500);
    }

    // ---- undo / redo ------------------------------------------------------------------------------
    /// A tool in the middle of something answers Ctrl+Z first.
    pub(super) fn on_undo(&mut self) {
        let handled = self
            .viewport
            .with_tool(|t, vp| t.on_undo(vp))
            .flatten()
            .unwrap_or(false);
        if handled {
            self.viewport.notify_scene_changed();
            return;
        }
        if self.viewport.undo() {
            self.viewport.notify_scene_changed();
        }
    }

    pub(super) fn on_redo(&mut self) {
        if self.viewport.redo() {
            self.viewport.notify_scene_changed();
        }
    }

    // ---- view ------------------------------------------------------------------------------------
    pub fn on_zoom_extents(&mut self) {
        if let Some((lo, hi)) = self.viewport.scene.bounds() {
            self.viewport.camera.fit_box(lo, hi, 1.1);
            self.viewport.update();
        }
    }

    pub(super) fn on_zoom_selection(&mut self) {
        match self.viewport.scene.selection_bounds() {
            None => self.show_message(&tr("Nothing selected", &[]), 2500),
            Some((lo, hi)) => {
                self.viewport.camera.fit_box(lo, hi, 1.1);
                self.viewport.update();
            }
        }
    }

    pub(super) fn on_standard_view(&mut self, key: &str) {
        self.viewport.camera.set_view(key);
        self.viewport.update();
    }

    /// Activate a built-in style (a copy — the presets stay pristine).
    pub(super) fn apply_display_style(&mut self, style: Style) {
        let name = style.name.clone();
        self.viewport.scene.display_style = style;
        self.sync_style_menu();
        self.viewport.update();
        self.show_message(&tr("Style: {name}", &[("name", &tr(&name, &[]))]), 2000);
    }

    /// Alt+X: into X-ray remembering the style left; out of it, back to that
    /// style — or Default when there is none for this document.
    pub(super) fn toggle_xray(&mut self) {
        let style = self.viewport.scene.display_style.clone();
        if style.face_mode == "xray" {
            let prev = self
                .style_before_xray
                .take()
                .filter(|(serial, _)| *serial == self.scene_serial)
                .map(|(_, s)| s)
                .or_else(|| style_by_name("Default"));
            if let Some(p) = prev {
                self.apply_display_style(p);
            }
        } else {
            self.style_before_xray = Some((self.scene_serial, style));
            if let Some(x) = style_by_name("X-ray") {
                self.apply_display_style(x);
            }
        }
    }

    pub(super) fn toggle_style_field(&mut self, field: &str) {
        let st = &mut self.viewport.scene.display_style;
        match field {
            "edges" => st.edges = !st.edges,
            "profiles" => st.profiles = !st.profiles,
            "back_edges" => st.back_edges = !st.back_edges,
            _ => st.section_fill = !st.section_fill,
        }
        self.sync_style_menu();
        self.viewport.update();
    }

    /// Camera ▸ Hidden Groups / Hidden Edges and Faces: a scene flag, a version bump so
    /// the pick index learns what became selectable; off, what the view no
    /// longer shows cannot stay selected.
    pub(super) fn set_hidden_view(&mut self, objects: bool, on: bool) {
        let scene = &mut self.viewport.scene;
        let cur = if objects {
            scene.show_hidden_objects
        } else {
            scene.show_hidden_geometry
        };
        if cur == on {
            return;
        }
        if objects {
            scene.show_hidden_objects = on;
        } else {
            scene.show_hidden_geometry = on;
        }
        if !on {
            let gone: Vec<EntityRef> = scene
                .selection
                .iter()
                .filter(|e| scene.entity_hidden(e) || is_hidden(scene, e))
                .copied()
                .collect();
            for e in gone {
                scene.selection.shift_remove(&e);
            }
        }
        scene.version += 1;
        self.sync_section_menu();
        self.viewport.update();
    }

    // ---- the eyedropper ----------------------------------------------------------------------------
    /// Arm (or cancel) a one-shot color sample; arming picks Paint too.
    pub(super) fn on_toggle_eyedropper(&mut self, on: bool) {
        set_paint_sample_armed(on);
        if let Some(a) = self.actions.get_mut("eyedropper") {
            a.checked = on;
        }
        if on {
            self.activate_tool("paint");
            // run last: its stack shows it, not the Paint it armed
            self.toolbar_stacks.follow("eyedropper");
            self.viewport
                .flash_status(&tr("Click a face to sample its color", &[]), 2500);
        }
        self.viewport.refresh_tool_cursor();
    }

    /// The tool sampled: the button pops out on its own.
    pub fn release_eyedropper(&mut self) {
        set_paint_sample_armed(false);
        if let Some(a) = self.actions.get_mut("eyedropper") {
            a.checked = false;
        }
        let _ = paint_sample_armed();
        self.viewport.refresh_tool_cursor();
    }

    // ---- sections ----------------------------------------------------------------------------------
    /// [`Self::prompt_section_name`], as the asker its answer comes back to.
    pub fn name_section_plane(&mut self, id: xtrude_model::ids::ObjId) {
        self.ask_as(super::Asker::SectionName(id), |w| w.prompt_section_name(id));
    }

    /// The post-placement prompt: name and symbol (cancel keeps the
    /// defaults; «Don't ask again» keeps them from then on).
    pub fn prompt_section_name(&mut self, id: xtrude_model::ids::ObjId) {
        if !settings::read(&keys::ASK_SECTION_NAME) {
            return;
        }
        let Some(p) = self
            .viewport
            .scene
            .doc
            .section_planes
            .iter()
            .find(|p| p.id == id)
        else {
            return;
        };
        let (name, symbol) = (p.name.clone(), p.symbol.clone());
        let Some(ans) = self.dialogs.section_name(&name, &symbol) else {
            return;
        };
        if let Some(p) = self
            .viewport
            .scene
            .doc
            .section_planes
            .iter_mut()
            .find(|p| p.id == id)
        {
            if !ans.name.trim().is_empty() {
                p.name = ans.name.trim().to_string();
            }
            if !ans.symbol.trim().is_empty() {
                p.symbol = ans.symbol.trim().chars().take(3).collect();
            }
        }
        if ans.dont_ask {
            settings::write(&keys::ASK_SECTION_NAME, &false);
        }
        self.viewport.update();
    }

    pub(super) fn selected_section_planes(&self) -> Vec<xtrude_model::ids::ObjId> {
        self.viewport
            .scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::SectionPlane(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    pub(super) fn on_reverse_section(&mut self) {
        for id in self.selected_section_planes() {
            self.viewport
                .run(xtrude_model::commands::ReverseSectionPlaneCommand { id });
        }
        self.viewport.update();
    }

    pub(super) fn on_toggle_active_section(&mut self) {
        let Some(&id) = self.selected_section_planes().first() else {
            return;
        };
        let active = self
            .viewport
            .scene
            .doc
            .section_planes
            .iter()
            .any(|p| p.id == id && p.active);
        self.viewport
            .run(xtrude_model::commands::SetActiveSectionCommand {
                id: if active { None } else { Some(id) },
            });
        self.viewport.update();
    }

    /// Align View: look straight at the cut face.
    pub(super) fn on_align_view_to_section(&mut self) {
        let scene = &self.viewport.scene;
        let plane = self
            .selected_section_planes()
            .first()
            .and_then(|id| scene.doc.section_planes.iter().find(|p| p.id == *id))
            .or_else(|| scene.active_section())
            .cloned();
        let Some(p) = plane else {
            return;
        };
        let cam = &mut self.viewport.camera;
        let n = p.normal;
        cam.target = p.point;
        cam.pitch = n.z.clamp(-1.0, 1.0).asin();
        if n.x.abs() > 1e-9 || n.y.abs() > 1e-9 {
            cam.yaw = n.y.atan2(n.x);
        }
        self.viewport.update();
    }
}
