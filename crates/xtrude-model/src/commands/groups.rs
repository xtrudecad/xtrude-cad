// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Group and component commands.

use std::collections::HashSet;

use xtrude_core::attrs::{FaceAttrs, Paint};
use xtrude_core::axes::{frame_axes, frame_matrix};
use xtrude_core::math::{DMat4, DVec3, MatExt};
use xtrude_core::mesh::{EdgeFlags, EdgeId, FaceId, Key, Mesh};
use xtrude_core::topology::tkey;

use crate::entity::EntityRef;
use crate::group::{
    carry_axes, iter_placements, make_independent, transformed_mesh, world_mesh, Group,
};
use crate::history::{CmdError, CmdResult, Command};
use crate::ids::{GroupId, MeshId};
use crate::materials::effective_attrs;
use crate::model_tree;
use crate::scene::{EditShare, Scene};

use super::mesh::{mirror_matrix, rotation_matrix, scale_matrix, ScaleFactor};

type FaceLoop = (Vec<DVec3>, Vec<Vec<DVec3>>, FaceAttrs);

fn key_set(pts: &[DVec3]) -> Vec<Key> {
    let mut k: Vec<Key> = pts.iter().map(|&p| tkey(p)).collect();
    k.sort();
    k.dedup();
    k
}

fn ukey(a: Key, b: Key) -> (Key, Key) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Encapsulate the selected faces and edges into a new Group with its own
/// mesh, removing them from the active mesh. `component` builds the mesh
/// in LOCAL coordinates and hands back an instance.
pub struct MakeGroupCommand {
    component: bool,
    name: Option<String>,
    face_loops: Vec<FaceLoop>,
    edge_ends: Vec<(DVec3, DVec3)>,
    flagged: Vec<(DVec3, DVec3, EdgeFlags)>,
    /// The group this command made.
    pub group: Option<GroupId>,
}

impl MakeGroupCommand {
    pub fn new(
        mesh: &Mesh,
        faces: &[FaceId],
        edges: &[EdgeId],
        component: bool,
        name: Option<&str>,
    ) -> Self {
        let faces: Vec<FaceId> = faces
            .iter()
            .copied()
            .filter(|&f| mesh.has_face(f))
            .collect();
        let edges: Vec<EdgeId> = edges
            .iter()
            .copied()
            .filter(|&e| mesh.has_edge(e))
            .collect();
        let face_loops = faces
            .iter()
            .map(|&f| {
                (
                    mesh.face_positions(f),
                    mesh.face_hole_positions(f),
                    mesh.face(f).attrs.clone(),
                )
            })
            .collect();
        let edge_ends = edges.iter().map(|&e| mesh.edge_ab(e)).collect();
        let mut flagged = Vec::new();
        let mut note = |e: EdgeId| {
            if !mesh.edge(e).is_plain() {
                let (a, b) = mesh.edge_ab(e);
                flagged.push((a, b, mesh.edge(e).flags()));
            }
        };
        for &f in &faces {
            for lp in mesh.face(f).loops() {
                let n = lp.len();
                for i in 0..n {
                    if let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) {
                        note(e);
                    }
                }
            }
        }
        for &e in &edges {
            note(e);
        }
        MakeGroupCommand {
            component,
            name: name.map(str::to_string),
            face_loops,
            edge_ends,
            flagged,
            group: None,
        }
    }

    fn build(&mut self, scene: &mut Scene) -> Group {
        let frame = scene.drawing_frame();
        let mut pts: Vec<DVec3> = Vec::new();
        for (lp, holes, _) in &self.face_loops {
            pts.extend(lp.iter().copied());
            for h in holes {
                pts.extend(h.iter().copied());
            }
        }
        for (a, b) in &self.edge_ends {
            pts.push(*a);
            pts.push(*b);
        }
        let mut origin = DVec3::ZERO;
        let mut rot: Option<(DVec3, DVec3, DVec3)> = None;
        let mut corner = DVec3::ZERO;
        if let (Some(fr), false) = (frame.as_ref(), pts.is_empty()) {
            let (fo, fx, fy, fz) = frame_axes(Some(fr));
            let loc = |p: DVec3| {
                let d = p - fo;
                DVec3::new(d.dot(fx), d.dot(fy), d.dot(fz))
            };
            let lo = pts
                .iter()
                .map(|&p| loc(p))
                .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(b));
            corner = fo + fx * lo.x + fy * lo.y + fz * lo.z;
            rot = Some((fx, fy, fz));
        }
        if self.component && !pts.is_empty() {
            origin = if rot.is_some() {
                corner
            } else {
                pts.iter()
                    .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b))
            };
        }
        let component = self.component;
        let local = |p: DVec3| -> DVec3 {
            if component {
                if let Some((x, y, z)) = rot {
                    let d = p - origin;
                    return DVec3::new(d.dot(x), d.dot(y), d.dot(z));
                }
            }
            p - origin
        };
        let mut gmesh = Mesh::new();
        for (lp, holes, attrs) in &self.face_loops {
            let o: Vec<DVec3> = lp.iter().map(|&p| local(p)).collect();
            let hs: Vec<Vec<DVec3>> = holes
                .iter()
                .map(|h| h.iter().map(|&p| local(p)).collect())
                .collect();
            let f = gmesh.add_face(&o, &hs);
            if !attrs.is_empty() {
                gmesh.faces[f].attrs.update(attrs);
            }
        }
        for &(a, b) in &self.edge_ends {
            let _ = gmesh.add_edge(local(a), local(b));
        }
        for (a, b, flags) in &self.flagged {
            if let (Some(va), Some(vb)) = (gmesh.vertex_at(local(*a)), gmesh.vertex_at(local(*b))) {
                if let Some(e) = gmesh.find_edge(va, vb) {
                    gmesh.edge_mut(e).stamp_flags(flags);
                }
            }
        }
        let mid = scene.doc.add_mesh(gmesh);
        let mut g = Group::new(mid, self.name.as_deref());
        if self.component {
            g.xform = Some(match rot {
                Some((x, y, z)) => frame_matrix(origin, x, y, z),
                None => DMat4::from_translation(origin),
            });
        } else if let Some((x, y, z)) = rot {
            g.axes = Some(frame_matrix(corner, x, y, z));
        }
        g
    }

    /// Take the grouped geometry off the active mesh.
    fn remove_from_active(&self, scene: &mut Scene) {
        let mid = scene.active_mesh_id();
        let face_keysets: HashSet<Vec<Key>> = self
            .face_loops
            .iter()
            .map(|(lp, _, _)| key_set(lp))
            .collect();
        let grouped: HashSet<Key> = self
            .face_loops
            .iter()
            .flat_map(|(lp, holes, _)| lp.iter().chain(holes.iter().flatten()).map(|&p| tkey(p)))
            .collect();
        let sel_edges: HashSet<(Key, Key)> = self
            .edge_ends
            .iter()
            .map(|&(a, b)| ukey(tkey(a), tkey(b)))
            .collect();
        let mut dropped = Vec::new();
        let m = scene.mesh_mut();
        for f in m.face_ids() {
            if face_keysets.contains(&key_set(&m.face_positions(f))) {
                m.remove_face(f);
                dropped.push(EntityRef::Face(mid, f));
            }
        }
        for e in m.edge_ids() {
            let (a, b) = m.edge_ab(e);
            let ek = ukey(tkey(a), tkey(b));
            if m.edge(e).faces.is_empty()
                && (sel_edges.contains(&ek)
                    || (grouped.contains(&tkey(a)) && grouped.contains(&tkey(b))))
            {
                m.remove_edge(e);
                dropped.push(EntityRef::Edge(mid, e));
            }
        }
        for d in dropped {
            scene.discard(&d);
        }
    }
}

/// Put a new group where new objects land: a child of the open group, its
/// matrix or mesh mapped through the inverse of the open group's matrix,
/// or the model's top level.
fn place_group(scene: &mut Scene, mut g: Group) -> CmdResult {
    let Some(cid) = scene.edit_group() else {
        scene.doc.groups.push(g);
        return Ok(());
    };
    let cx = scene.doc.find_group(cid).and_then(|c| c.xform);
    if let Some(inv) = cx.and_then(|x| x.try_inverse()) {
        if let Some(gx) = g.xform {
            g.xform = Some(inv * gx);
        } else {
            let tm = transformed_mesh(scene.doc.mesh(g.mesh), &inv);
            g.mesh = scene.doc.add_mesh(tm);
            carry_axes(&mut g, &inv);
        }
    }
    let c = scene
        .doc
        .find_group_mut(cid)
        .ok_or_else(|| CmdError::msg("open group is gone"))?;
    c.children.push(g);
    Ok(())
}

/// A group made inside an open component is part of its contents: it gets
/// a matrix and is no component of its own, as `copy_group` gives such
/// children, so the copies keep matching.
fn place_in_definition(scene: &Scene, g: &mut Group) {
    if scene.in_definition() && g.xform.is_none() {
        g.xform = Some(DMat4::IDENTITY);
        g.component = false;
    }
}

/// The groups the open group holds, or the model's top level.
fn open_siblings(scene: &mut Scene) -> Option<&mut Vec<Group>> {
    match scene.edit_group() {
        Some(cid) => scene.doc.find_group_mut(cid).map(|c| &mut c.children),
        None => Some(&mut scene.doc.groups),
    }
}

impl Command for MakeGroupCommand {
    fn name(&self) -> &str {
        "MakeGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut g = self.build(scene);
        self.remove_from_active(scene);
        place_in_definition(scene, &mut g);
        let id = g.id;
        place_group(scene, g)?;
        self.group = Some(id);
        scene.selection.clear();
        scene.selection.insert(EntityRef::Group(id));
        scene.version += 1;
        Ok(())
    }
}

/// Leaving a component instance edited in place: the session's edits go
/// into the shared definition.
pub struct ReshareInstanceCommand {
    pub group: GroupId,
    pub proto: MeshId,
    pub xform: DMat4,
    pub edited: MeshId,
}

impl Command for ReshareInstanceCommand {
    fn name(&self) -> &str {
        "ReshareInstanceCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.share_back(self.group, self.proto, self.xform, self.edited);
        scene.version += 1;
        Ok(())
    }
}

/// Leaving a container component copy edited in place: its own faces go
/// into the shared definition, and the copy is put back on its matrix.
pub struct ShareContentsCommand {
    pub share: EditShare,
}

impl Command for ShareContentsCommand {
    fn name(&self) -> &str {
        "ShareContentsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.share_back_contents(&self.share);
        scene.version += 1;
        Ok(())
    }
}

/// Make Independent: bake an instance into its OWN mesh.
pub struct MakeIndependentCommand {
    pub group: GroupId,
}

impl Command for MakeIndependentCommand {
    fn name(&self) -> &str {
        "MakeIndependentCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut meshes = std::mem::take(&mut scene.doc.meshes);
        if let Some(g) = scene.doc.find_group_mut(self.group) {
            make_independent(&mut meshes, g);
        }
        scene.doc.meshes = meshes;
        scene.version += 1;
        Ok(())
    }
}

/// Convert a CLASSIC group into a component instance in place.
pub struct GroupToComponentCommand {
    pub group: GroupId,
    pub name: Option<String>,
}

impl Command for GroupToComponentCommand {
    fn name(&self) -> &str {
        "GroupToComponentCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let g = scene
            .doc
            .find_group_mut(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        if g.xform.is_none() {
            g.xform = Some(DMat4::IDENTITY);
        }
        g.component = true;
        if let Some(n) = self.name.as_ref().filter(|n| !n.is_empty()) {
            g.name = n.clone();
        }
        scene.version += 1;
        Ok(())
    }
}

/// Insert a ready-made Group into the scene (into the open container when
/// one is open), selected.
pub struct InsertGroupCommand {
    pub group: Option<Group>,
    pub id: GroupId,
}

impl InsertGroupCommand {
    pub fn new(g: Group) -> Self {
        let id = g.id;
        InsertGroupCommand { group: Some(g), id }
    }
}

impl Command for InsertGroupCommand {
    fn name(&self) -> &str {
        "InsertGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let Some(g) = self.group.take() else {
            return Ok(());
        };
        place_group(scene, g)?;
        scene.selection.clear();
        scene.selection.insert(EntityRef::Group(self.id));
        scene.version += 1;
        Ok(())
    }
}

/// Make Group when the selection already holds groups: one container,
/// holding the loose part as its own mesh and the groups as children.
pub struct MakeNestedGroupCommand {
    inner: Option<MakeGroupCommand>,
    pub groups: Vec<GroupId>,
    pub name: Option<String>,
    pub container: Option<GroupId>,
}

impl MakeNestedGroupCommand {
    pub fn new(
        mesh: &Mesh,
        faces: &[FaceId],
        edges: &[EdgeId],
        groups: Vec<GroupId>,
        name: Option<&str>,
    ) -> Self {
        let inner = (!faces.is_empty() || !edges.is_empty())
            .then(|| MakeGroupCommand::new(mesh, faces, edges, false, name));
        MakeNestedGroupCommand {
            inner,
            groups,
            name: name.map(str::to_string),
            container: None,
        }
    }
}

impl Command for MakeNestedGroupCommand {
    fn name(&self) -> &str {
        "MakeNestedGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let cid = if let Some(inner) = self.inner.as_mut() {
            inner.apply(scene)?;
            inner.group.ok_or_else(|| CmdError::msg("no container"))?
        } else {
            let mid = scene.doc.add_mesh(Mesh::new());
            let mut g = Group::new(mid, self.name.as_deref());
            place_in_definition(scene, &mut g);
            let id = g.id;
            place_group(scene, g)?;
            id
        };
        self.container = Some(cid);
        // The groups to nest are the container's siblings, taken in
        // document order, not selection order.
        let siblings = open_siblings(scene).ok_or_else(|| CmdError::msg("open group is gone"))?;
        let mut kids = Vec::new();
        let mut i = 0;
        while i < siblings.len() {
            if self.groups.contains(&siblings[i].id) {
                kids.push(siblings.remove(i));
            } else {
                i += 1;
            }
        }
        for k in &kids {
            scene.discard(&EntityRef::Group(k.id));
        }
        let c = scene
            .doc
            .find_group_mut(cid)
            .ok_or_else(|| CmdError::msg("container is gone"))?;
        c.adopt(kids);
        c.component = false;
        scene.selection.clear();
        scene.selection.insert(EntityRef::Group(cid));
        scene.version += 1;
        Ok(())
    }
}

/// Make Component over a selection holding groups: ONE component
/// containing them.
pub struct MakeComponentOfCommand {
    pub nest: MakeNestedGroupCommand,
}

impl Command for MakeComponentOfCommand {
    fn name(&self) -> &str {
        "MakeComponentOfCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        self.nest.apply(scene)?;
        if let Some(c) = self
            .nest
            .container
            .and_then(|c| scene.doc.find_group_mut(c))
        {
            c.component = true;
        }
        scene.version += 1;
        Ok(())
    }
}

/// Fuse several groups into ONE new group holding their combined
/// world-space geometry.
pub struct MergeGroupsCommand {
    pub groups: Vec<GroupId>,
    pub merged: Option<GroupId>,
}

impl Command for MergeGroupsCommand {
    fn name(&self) -> &str {
        "MergeGroupsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut combined = Mesh::new();
        let mut first_name = None;
        let mut first_layer = None;
        let mut srcs = Vec::new();
        for gid in &self.groups {
            let g = scene
                .doc
                .find_group(*gid)
                .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
            if first_name.is_none() {
                first_name = Some(g.name.clone());
                first_layer = Some(g.layer.clone());
            }
            srcs.push(world_mesh(&scene.doc.meshes, g));
        }
        // Every group's edges weld first, then every face — the bulk
        // loading order.
        for src in &srcs {
            for (eid, e) in src.edges.iter() {
                let (a, b) = src.edge_ab(eid);
                if let Ok(ne) = combined.add_edge(a, b) {
                    let ed = combined.edge_mut(ne);
                    if e.soft {
                        ed.soft = true;
                    }
                    if e.hidden {
                        ed.hidden = true;
                    }
                    if e.curve.is_some() {
                        ed.curve = e.curve;
                    }
                    if e.layer.is_some() {
                        ed.layer = e.layer.clone();
                    }
                }
            }
        }
        for src in &srcs {
            for (fid, f) in src.faces.iter() {
                let nf = combined.add_face(&src.face_positions(fid), &src.face_hole_positions(fid));
                if !f.attrs.is_empty() {
                    combined.faces[nf].attrs = f.attrs.clone();
                }
            }
        }
        let mid = scene.doc.add_mesh(combined);
        let mut merged = Group::new(mid, first_name.as_deref());
        merged.layer = first_layer.flatten();
        let id = merged.id;
        for gid in &self.groups {
            scene.doc.groups.retain(|g| g.id != *gid);
            scene.discard(&EntityRef::Group(*gid));
        }
        scene.doc.groups.push(merged);
        self.merged = Some(id);
        scene.selection.clear();
        scene.selection.insert(EntityRef::Group(id));
        scene.version += 1;
        Ok(())
    }
}

/// Paint a group or component instance as a whole (`None` unpaints).
pub struct SetGroupMaterialCommand {
    pub group: GroupId,
    pub material: Option<Paint>,
}

impl Command for SetGroupMaterialCommand {
    fn name(&self) -> &str {
        "SetGroupMaterialCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(g) = scene.doc.find_group_mut(self.group) {
            g.material = self.material.clone().filter(|p| !p.is_empty());
        }
        scene.version += 1;
        Ok(())
    }
}

/// Dissolve ONE level of a group: its own geometry merges back into the
/// active mesh and the nested groups come out whole.
pub struct ExplodeGroupCommand {
    pub group: GroupId,
}

impl Command for ExplodeGroupCommand {
    fn name(&self) -> &str {
        "ExplodeGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let index = scene
            .doc
            .groups
            .iter()
            .position(|g| g.id == self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        let g = scene.doc.groups[index].clone();
        let p = g.xform;
        let paint = g.material.clone();
        let w = |q: DVec3| p.map_or(q, |m| m.project_point(q));
        let src = scene.doc.mesh(g.mesh).clone();
        {
            let m = scene.mesh_mut();
            for (fid, f) in src.faces.iter() {
                let outer: Vec<DVec3> = src.face_positions(fid).into_iter().map(w).collect();
                let holes: Vec<Vec<DVec3>> = src
                    .face_hole_positions(fid)
                    .into_iter()
                    .map(|h| h.into_iter().map(w).collect())
                    .collect();
                let nf = m.add_face(&outer, &holes);
                let drawn = effective_attrs(&f.attrs, paint.as_ref());
                m.faces[nf].attrs.update(&drawn);
            }
            for (eid, _) in src.edges.iter() {
                let (a, b) = src.edge_ab(eid);
                let (wa, wb) = (w(a), w(b));
                let exists = match (m.vertex_at(wa), m.vertex_at(wb)) {
                    (Some(v0), Some(v1)) => m.find_edge(v0, v1).is_some(),
                    _ => false,
                };
                if !exists {
                    let _ = m.add_edge(wa, wb);
                }
            }
            for (eid, e) in src.edges.iter() {
                if e.is_plain() {
                    continue;
                }
                let (a, b) = src.edge_ab(eid);
                if let (Some(v0), Some(v1)) = (m.vertex_at(w(a)), m.vertex_at(w(b))) {
                    if let Some(k) = m.find_edge(v0, v1) {
                        m.edge_mut(k).stamp_flags(&e.flags());
                    }
                }
            }
            m.resplit_curves();
        }
        let mut kids = g.children.clone();
        for c in kids.iter_mut() {
            c.explode_offset = None;
            if let Some(pm) = p {
                if let Some(cx) = c.xform {
                    c.xform = Some(pm * cx);
                } else {
                    let tm = transformed_mesh(scene.doc.mesh(c.mesh), &pm);
                    c.mesh = scene.doc.add_mesh(tm);
                    carry_axes(c, &pm);
                }
            }
            if c.material.is_none() {
                if let Some(pp) = &paint {
                    c.material = Some(pp.clone());
                }
            }
        }
        let kid_ids: Vec<GroupId> = kids.iter().map(|k| k.id).collect();
        scene.doc.groups.remove(index);
        for (i, k) in kids.into_iter().enumerate() {
            scene.doc.groups.insert(index + i, k);
        }
        scene.discard(&EntityRef::Group(self.group));
        if !kid_ids.is_empty() {
            scene.selection.clear();
            scene
                .selection
                .extend(kid_ids.into_iter().map(EntityRef::Group));
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct RenameGroupCommand {
    pub group: GroupId,
    pub name: String,
}

impl Command for RenameGroupCommand {
    fn name(&self) -> &str {
        "RenameGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(g) = scene.doc.find_group_mut(self.group) {
            g.name = self.name.clone();
        }
        scene.version += 1;
        Ok(())
    }
}

/// Move groups under another group, or out to the top level (the
/// model tree's drag and drop). Nothing moves in the world.
pub struct ReparentGroupsCommand {
    pub groups: Vec<GroupId>,
    pub parent: Option<GroupId>,
    pub index: Option<usize>,
}

impl Command for ReparentGroupsCommand {
    fn name(&self) -> &str {
        "ReparentGroupsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let parents = model_tree::parent_map(scene);
        let new_chain: Vec<GroupId> = match self.parent {
            None => Vec::new(),
            Some(p) => {
                let mut c = model_tree::ancestors(&parents, p);
                c.push(p);
                c
            }
        };
        let target = model_tree::chain_matrix(scene, &new_chain);
        let inv = match target {
            Some(t) => {
                let Some(inv) = t.try_inverse() else {
                    return Err(CmdError::msg("the target frame cannot be inverted"));
                };
                Some(inv)
            }
            None => None,
        };
        // World placements and old positions first.
        let mut moves: Vec<(GroupId, Option<GroupId>, usize, Option<DMat4>)> = Vec::new();
        for &g in &self.groups {
            let old_parent = parents.get(&g).copied().flatten();
            let mut chain = model_tree::ancestors(&parents, g);
            chain.push(g);
            let world = model_tree::chain_matrix(scene, &chain);
            let list = scene
                .doc
                .siblings_mut(old_parent)
                .ok_or_else(|| CmdError::msg("parent is gone"))?;
            let idx = list
                .iter()
                .position(|x| x.id == g)
                .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
            moves.push((g, old_parent, idx, world));
        }
        // Take them out (back to front within each list).
        let mut order: Vec<usize> = (0..moves.len()).collect();
        order.sort_by(|&a, &b| {
            (moves[a].1, std::cmp::Reverse(moves[a].2))
                .cmp(&(moves[b].1, std::cmp::Reverse(moves[b].2)))
        });
        let mut taken: Vec<Option<Group>> = vec![None; moves.len()];
        for i in order {
            let (gid, op, _, _) = moves[i];
            let list = scene
                .doc
                .siblings_mut(op)
                .ok_or_else(|| CmdError::msg("parent is gone"))?;
            if let Some(pos) = list.iter().position(|x| x.id == gid) {
                taken[i] = Some(list.remove(pos));
            }
        }
        let new_len = scene.doc.siblings_mut(self.parent).map_or(0, |l| l.len());
        let mut index = self.index.unwrap_or(new_len + moves.len());
        if self.index.is_some() {
            index -= moves
                .iter()
                .filter(|(_, op, i, _)| *op == self.parent && *i < self.index.unwrap_or(0))
                .count()
                .min(index);
        }
        let index = index.min(new_len);
        let mut to_insert = Vec::new();
        for (k, g) in taken.into_iter().enumerate() {
            let Some(mut g) = g else { continue };
            let world = moves[k].3;
            let local = match (world, inv) {
                (None, None) => None,
                (None, Some(i)) => Some(i),
                (Some(w), None) => Some(w),
                (Some(w), Some(i)) => Some(i * w),
            };
            if g.xform.is_none() && local.is_some() {
                g.component = false;
            }
            g.xform = local;
            g.explode_offset = None;
            scene.discard(&EntityRef::Group(g.id));
            to_insert.push(g);
        }
        let list = scene
            .doc
            .siblings_mut(self.parent)
            .ok_or_else(|| CmdError::msg("target parent is gone"))?;
        for (off, g) in to_insert.into_iter().enumerate() {
            let at = (index + off).min(list.len());
            list.insert(at, g);
        }
        if let Some(p) = self.parent {
            if let Some(pg) = scene.doc.find_group_mut(p) {
                if pg.xform.is_none() {
                    pg.xform = Some(DMat4::IDENTITY);
                    pg.component = false;
                }
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Pull a component's parts apart to `factor` (0 reassembles).
pub struct ExplodeViewCommand {
    pub container: GroupId,
    pub factor: f64,
    pub mode: String,
}

impl Command for ExplodeViewCommand {
    fn name(&self) -> &str {
        "ExplodeViewCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let meshes = scene.doc.meshes.clone();
        if let Some(c) = scene.doc.find_group_mut(self.container) {
            crate::explode::apply_explode(&meshes, c, self.factor, &self.mode, None);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Replace what `group` holds with `pieces` (one child per physical piece).
pub struct SplitIntoPiecesCommand {
    pub group: GroupId,
    pub pieces: Vec<Group>,
}

impl Command for SplitIntoPiecesCommand {
    fn name(&self) -> &str {
        "SplitIntoPiecesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let empty = scene.doc.add_mesh(Mesh::new());
        let pieces = self.pieces.clone();
        let g = scene
            .doc
            .find_group_mut(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        g.mesh = empty;
        g.adopt(pieces);
        g.exploded = None;
        scene.version += 1;
        Ok(())
    }
}

/// Translate a whole group (compose into an instance's xform, or move a
/// classic group's vertices and carry its axes).
pub struct MoveGroupCommand {
    pub group: GroupId,
    pub delta: DVec3,
}

impl Command for MoveGroupCommand {
    fn name(&self) -> &str {
        "MoveGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let t = DMat4::from_translation(self.delta);
        let g = scene
            .doc
            .find_group(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        if g.xform.is_some() {
            let g = scene.doc.find_group_mut(self.group).expect("found above");
            g.xform = Some(t * g.xform.unwrap_or(DMat4::IDENTITY));
        } else {
            let mid = g.mesh;
            let m = scene.doc.mesh_mut(mid);
            for v in m.vertex_ids() {
                m.move_vertex(v, self.delta);
            }
            if let Some(g) = scene.doc.find_group_mut(self.group) {
                carry_axes(g, &t);
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Rotate a whole group (rigid).
pub struct RotateGroupCommand {
    pub group: GroupId,
    pub center: DVec3,
    pub axis: DVec3,
    pub degrees: f64,
}

impl Command for RotateGroupCommand {
    fn name(&self) -> &str {
        "RotateGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let m = rotation_matrix(self.center, self.axis, self.degrees);
        transform_group(scene, self.group, &m, false)
    }
}

/// Scale a whole group about `center`.
pub struct ScaleGroupCommand {
    pub group: GroupId,
    pub center: DVec3,
    pub factor: ScaleFactor,
    pub axes: Option<(DVec3, DVec3, DVec3)>,
}

impl Command for ScaleGroupCommand {
    fn name(&self) -> &str {
        "ScaleGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let m = scale_matrix(self.center, self.factor, self.axes);
        transform_group(scene, self.group, &m, false)
    }
}

/// Flip whole groups about an axis plane; classic groups re-reverse their
/// face loops so the mirrored solid keeps its faces pointing OUT.
pub struct FlipGroupsCommand {
    pub groups: Vec<GroupId>,
    pub centre: DVec3,
    pub axis: DVec3,
}

impl Command for FlipGroupsCommand {
    fn name(&self) -> &str {
        "FlipGroupsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let m = mirror_matrix(self.centre, self.axis);
        for &g in &self.groups.clone() {
            transform_group(scene, g, &m, true)?;
        }
        Ok(())
    }
}

/// Apply `m` to a group: compose for an instance, map the vertices (and
/// carry the axes) for a classic group; `reverse` re-reverses the loops.
pub fn transform_group(scene: &mut Scene, gid: GroupId, m: &DMat4, reverse: bool) -> CmdResult {
    let g = scene
        .doc
        .find_group(gid)
        .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
    if g.xform.is_some() {
        let g = scene.doc.find_group_mut(gid).expect("found above");
        g.xform = Some(*m * g.xform.unwrap_or(DMat4::IDENTITY));
        scene.version += 1;
        return Ok(());
    }
    let mid = g.mesh;
    let mesh = scene.doc.mesh_mut(mid);
    for v in mesh.vertex_ids() {
        let p = mesh.pos(v);
        mesh.move_vertex(v, m.project_point(p) - p);
    }
    if reverse {
        for f in mesh.face_ids() {
            let fc = mesh.face_mut(f);
            fc.outer.reverse();
            for h in fc.holes.iter_mut() {
                h.reverse();
            }
        }
    }
    if let Some(g) = scene.doc.find_group_mut(gid) {
        carry_axes(g, m);
    }
    scene.version += 1;
    Ok(())
}

/// Remove a whole group from wherever it lives.
pub struct DeleteGroupCommand {
    pub group: GroupId,
}

impl Command for DeleteGroupCommand {
    fn name(&self) -> &str {
        "DeleteGroupCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene
            .doc
            .take_group(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        scene.discard(&EntityRef::Group(self.group));
        scene.version += 1;
        Ok(())
    }
}

/// Change Axes: give `group` a new frame while nothing moves in the world.
pub struct ChangeAxesCommand {
    pub group: GroupId,
    pub frame: DMat4,
}

impl Command for ChangeAxesCommand {
    fn name(&self) -> &str {
        "ChangeAxesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let g = scene
            .doc
            .find_group(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?;
        let Some(gx) = g.xform else {
            let g = scene.doc.find_group_mut(self.group).expect("found above");
            g.axes = Some(self.frame);
            scene.version += 1;
            return Ok(());
        };
        let proto = g.mesh;
        let Some(inv) = self.frame.try_inverse() else {
            return Ok(());
        };
        let d = inv * gx;
        let Some(d_inv) = d.try_inverse() else {
            return Ok(());
        };
        // Every instance of the definition.
        let mut instances: Vec<GroupId> = Vec::new();
        for top in &scene.doc.groups {
            for (pg, _) in iter_placements(top, None) {
                if pg.mesh == proto && pg.xform.is_some() && !instances.contains(&pg.id) {
                    instances.push(pg.id);
                }
            }
        }
        if !instances.contains(&self.group) {
            instances.push(self.group);
        }
        let moved = transformed_mesh(scene.doc.mesh(proto), &d);
        *scene.doc.mesh_mut(proto) = moved;
        if let Some(g) = scene.doc.find_group_mut(self.group) {
            for c in g.children.iter_mut() {
                if let Some(cx) = c.xform {
                    c.xform = Some(d * cx);
                }
            }
        }
        for id in instances {
            if let Some(inst) = scene.doc.find_group_mut(id) {
                if let Some(x) = inst.xform {
                    inst.xform = Some(x * d_inv);
                }
            }
        }
        scene.version += 1;
        Ok(())
    }
}
