// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Copy / Cut into the model clipboard.

use glam::DVec3;
use xtrude_io::clip::ClipData;
use xtrude_model::commands::{DeleteGroupCommand, EraseSelectionCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{copy_group, iter_placements, placement_points, MeshTable};
use xtrude_model::history::{Command, CompoundCommand};

use super::{Viewport, ViewportEvent};

impl Viewport {
    /// Copy the selected faces, edges and groups into the clipboard (as
    /// positions, with a reference corner). False when nothing is
    /// selected.
    pub fn copy_selection(&mut self) -> bool {
        let mid = self.scene.active_mesh_id();
        let (mut faces, mut edges, mut groups) = (Vec::new(), Vec::new(), Vec::new());
        for e in self.scene.selection.iter() {
            match *e {
                EntityRef::Face(m, f) if m == mid => faces.push(f),
                EntityRef::Edge(m, x) if m == mid => edges.push(x),
                EntityRef::Group(g) => groups.push(g),
                _ => {}
            }
        }
        if faces.is_empty() && edges.is_empty() && groups.is_empty() {
            return false;
        }
        let mesh = self.scene.mesh();
        // Attrs travel with each face — a copy, so later re-paints of the
        // original never touch the clipboard snapshot.
        let face_data: Vec<_> = faces
            .iter()
            .filter(|&&f| mesh.has_face(f))
            .map(|&f| {
                (
                    mesh.face_positions(f),
                    mesh.face_hole_positions(f),
                    mesh.face(f).attrs.clone(),
                )
            })
            .collect();
        // Soft/curve flags keep a pasted circle ONE curve (ids remapped at
        // paste time).
        let edge_data: Vec<_> = edges
            .iter()
            .filter(|&&e| mesh.has_edge(e))
            .map(|&e| {
                let (a, b) = mesh.edge_ab(e);
                let ed = mesh.edge(e);
                (a, b, ed.soft, ed.curve)
            })
            .collect();
        // Groups are snapshotted NOW (instances keep sharing their
        // prototype; classic groups deep-copy). A classic group's snapshot
        // becomes an identity INSTANCE of its fresh mesh, still a GROUP:
        // every stamp is an O(1) sibling.
        let mut table: MeshTable = self.scene.doc.meshes.clone();
        let mut group_data = Vec::new();
        for g in &groups {
            let Some(src) = self.scene.doc.find_group(*g) else {
                continue;
            };
            let mut src = src.clone();
            let mut tpl = copy_group(&mut table, &mut src, None, false);
            if tpl.xform.is_none() {
                tpl.xform = Some(glam::DMat4::IDENTITY);
                tpl.component = false;
            }
            group_data.push(tpl);
        }
        let mut clip_meshes = MeshTable::new();
        for tpl in &group_data {
            for (pg, _) in iter_placements(tpl, None) {
                if let Some(m) = table.get(&pg.mesh) {
                    clip_meshes.insert(pg.mesh, m.clone());
                }
            }
        }
        // Reference corner: min corner of everything copied, containers
        // through their parts (an imported lamp post's own mesh is empty).
        let mut lo: Option<DVec3> = None;
        let mut absorb = |p: DVec3| lo = Some(lo.map_or(p, |l| l.min(p)));
        for (lp, holes, _) in &face_data {
            lp.iter()
                .chain(holes.iter().flatten())
                .for_each(|p| absorb(*p));
        }
        for (a, b, _, _) in &edge_data {
            absorb(*a);
            absorb(*b);
        }
        for tpl in &group_data {
            placement_points(&clip_meshes, tpl)
                .into_iter()
                .for_each(&mut absorb);
        }
        let Some(reference) = lo else {
            return false;
        };
        self.clipboard = Some(ClipData {
            faces: face_data,
            edges: edge_data,
            groups: group_data,
            meshes: clip_meshes,
            reference,
        });
        // The window also offers it on the system clipboard.
        self.outbox.events.push(ViewportEvent::ClipboardChanged);
        true
    }

    /// Copy the selection, then erase it (one undoable step).
    pub fn cut_selection(&mut self) -> bool {
        if !self.copy_selection() {
            return false;
        }
        let mid = self.scene.active_mesh_id();
        let (mut faces, mut edges, mut groups) = (Vec::new(), Vec::new(), Vec::new());
        for e in self.scene.selection.iter() {
            match *e {
                EntityRef::Face(m, f) if m == mid => faces.push(f),
                EntityRef::Edge(m, x) if m == mid => edges.push(x),
                EntityRef::Group(g) => groups.push(g),
                _ => {}
            }
        }
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        if !edges.is_empty() || !faces.is_empty() {
            cmds.push(Box::new(EraseSelectionCommand::from_entities(
                self.scene.mesh(),
                &edges,
                &faces,
            )));
        }
        for g in groups {
            cmds.push(Box::new(DeleteGroupCommand { group: g }));
        }
        match cmds.len() {
            0 => {}
            1 => {
                let c = cmds.pop().expect("one command");
                self.execute_boxed(c);
            }
            _ => {
                self.execute(&mut CompoundCommand::new(cmds));
            }
        }
        self.update();
        true
    }
}
