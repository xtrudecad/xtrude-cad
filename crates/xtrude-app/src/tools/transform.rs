// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What the transform tools (Move, Rotate, Scale, Flip) act on, and the
//! copies they stamp.

use std::collections::HashSet;

use glam::{DMat4, DVec3};
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::math::MatExt;
use xtrude_core::topology::tkey;
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::attrs::DimMoveMode;
use xtrude_model::commands::{
    AddEdgeCommand, AddFaceCommand, InsertGroupCommand, RotateGroupCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{copy_group, iter_placements};
use xtrude_model::history::{CmdResult, Command};
use xtrude_model::ids::{GroupId, ObjId};
use xtrude_model::scene::Scene;

use super::ToolContext;
use crate::viewport::Viewport;

/// A face as it was when the operation started (copies stamp these).
#[derive(Clone, Debug)]
pub struct FaceSnap {
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub attrs: FaceAttrs,
}

/// An edge as it was when the operation started.
#[derive(Clone, Copy, Debug)]
pub struct EdgeSnap {
    pub a: DVec3,
    pub b: DVec3,
    pub soft: bool,
    pub curve: Option<i64>,
}

/// Unique positions (by the topology key), first occurrence first.
pub fn dedup(positions: Vec<DVec3>) -> Vec<DVec3> {
    let mut seen = HashSet::new();
    positions
        .into_iter()
        .filter(|p| seen.insert(tkey(*p)))
        .collect()
}

/// What a transform tool acts on: `(groups, positions)` — every selected
/// (or the hovered) group as a unit, and the unique loose coordinates of
/// the selection or of the entity under the cursor.
pub fn gather_targets(vp: &mut Viewport, ctx: &ToolContext) -> (Vec<GroupId>, Vec<DVec3>) {
    let mut entities: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
    if entities.is_empty() {
        if let Some(g) = vp.pick_group(ctx.screen.0, ctx.screen.1) {
            return (vec![g], Vec::new());
        }
        if let Some(e) = vp
            .pick_edge(ctx.screen.0, ctx.screen.1)
            .and_then(|e| e.edge)
        {
            entities = vec![EntityRef::Edge(e.0, e.1)];
        } else if let Some((m, f)) = vp.pick_face(ctx.screen.0, ctx.screen.1) {
            entities = vec![EntityRef::Face(m, f)];
        }
    }
    let groups: Vec<GroupId> = entities.iter().filter_map(|e| e.as_group()).collect();
    let mut positions = Vec::new();
    for e in &entities {
        match *e {
            EntityRef::Edge(m, id) => {
                let mesh = vp.scene.doc.mesh(m);
                if mesh.has_edge(id) {
                    let (a, b) = mesh.edge_ab(id);
                    positions.extend([a, b]);
                }
            }
            EntityRef::Face(m, id) => {
                let mesh = vp.scene.doc.mesh(m);
                if mesh.has_face(id) {
                    positions.extend(mesh.face_positions(id));
                }
            }
            _ => {}
        }
    }
    (groups, dedup(positions))
}

/// The dimensions Move acts on, each with how: dimensions riding on
/// vertices that move anyway are left alone.
pub fn gather_dimensions(
    vp: &mut Viewport,
    ctx: &ToolContext,
    groups: &[GroupId],
    verts: &[xtrude_core::VertexId],
) -> Vec<(ObjId, DimMoveMode)> {
    let mut dims: Vec<ObjId> = vp
        .scene
        .selection
        .iter()
        .filter_map(|e| match *e {
            EntityRef::Dimension(d) => Some(d),
            _ => None,
        })
        .collect();
    if dims.is_empty() && vp.scene.selection.is_empty() {
        if let Some(d) = vp.pick_dimension(ctx.screen.0, ctx.screen.1) {
            dims.push(d);
        }
    }
    let active = vp.scene.active_mesh_id();
    let mut out = Vec::new();
    for id in dims {
        let Some(d) = vp.scene.doc.dimensions.iter().find(|d| d.id == id) else {
            continue;
        };
        let anchors: Vec<_> = [&d.anchor_a, &d.anchor_b].into_iter().flatten().collect();
        let riding = anchors
            .iter()
            .filter(|an| {
                (an.mesh == active && verts.contains(&an.vertex))
                    || an.chain.iter().any(|g| groups.contains(g))
            })
            .count();
        if anchors.len() == 2 && riding == 2 {
            continue;
        }
        out.push((
            id,
            if anchors.len() == 2 {
                DimMoveMode::Line
            } else {
                DimMoveMode::Rigid
            },
        ));
    }
    out
}

/// The leader texts Move acts on: the selected ones, or the one whose
/// text block is under the cursor.
pub fn gather_labels(vp: &mut Viewport, ctx: &ToolContext) -> Vec<ObjId> {
    let mut labels: Vec<ObjId> = vp
        .scene
        .selection
        .iter()
        .filter_map(|e| match *e {
            EntityRef::TextLabel(t) => Some(t),
            _ => None,
        })
        .collect();
    if labels.is_empty() && vp.scene.selection.is_empty() {
        if let Some(l) = vp.pick_text_label(ctx.screen.0, ctx.screen.1, true) {
            labels.push(l);
        }
    }
    labels
}

/// The selected section planes.
pub fn selected_section_planes(vp: &Viewport) -> Vec<ObjId> {
    vp.scene
        .selection
        .iter()
        .filter_map(|e| match *e {
            EntityRef::SectionPlane(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// The faces / edges a copy duplicates and the wireframe its preview
/// carries (the groups' placements, or the loose selection — or the
/// loose entity under the cursor).
pub fn gather_copy_entities(
    vp: &mut Viewport,
    ctx: &ToolContext,
    groups: &[GroupId],
) -> (Vec<FaceSnap>, Vec<EdgeSnap>, Vec<(DVec3, DVec3)>) {
    let mut segs = Vec::new();
    for &gid in groups {
        let Some(g) = vp.scene.doc.find_group(gid) else {
            continue;
        };
        for (pg, xf) in iter_placements(g, None) {
            let mesh = vp.scene.doc.mesh(pg.mesh);
            for e in mesh.edge_ids() {
                let (a, b) = mesh.edge_ab(e);
                segs.push(match xf {
                    Some(m) => (m.project_point(a), m.project_point(b)),
                    None => (a, b),
                });
            }
        }
    }
    let mut sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
    if sel.is_empty() && groups.is_empty() {
        if let Some(e) = vp
            .pick_edge(ctx.screen.0, ctx.screen.1)
            .and_then(|e| e.edge)
        {
            sel = vec![EntityRef::Edge(e.0, e.1)];
        } else if let Some((m, f)) = vp.pick_face(ctx.screen.0, ctx.screen.1) {
            sel = vec![EntityRef::Face(m, f)];
        }
    }
    let mut faces = Vec::new();
    let mut edges = Vec::new();
    for e in &sel {
        if let EntityRef::Face(m, f) = *e {
            let mesh = vp.scene.doc.mesh(m);
            if mesh.has_face(f) {
                faces.push(face_snap(mesh, f));
            }
        }
    }
    for e in &sel {
        if let EntityRef::Edge(m, id) = *e {
            let mesh = vp.scene.doc.mesh(m);
            if mesh.has_edge(id) {
                let (a, b) = mesh.edge_ab(id);
                let ed = mesh.edge(id);
                edges.push(EdgeSnap {
                    a,
                    b,
                    soft: ed.soft,
                    curve: ed.curve,
                });
            }
        }
    }
    for f in &faces {
        for lp in std::iter::once(&f.outer).chain(f.holes.iter()) {
            let n = lp.len();
            for i in 0..n {
                segs.push((lp[i], lp[(i + 1) % n]));
            }
        }
    }
    for e in &edges {
        segs.push((e.a, e.b));
    }
    (faces, edges, segs)
}

pub fn face_snap(mesh: &Mesh, f: FaceId) -> FaceSnap {
    FaceSnap {
        outer: mesh.face_positions(f),
        holes: mesh.face_hole_positions(f),
        attrs: mesh.face(f).attrs.clone(),
    }
}

/// Which command a hot retype / array may replace: the history depth and
/// scene version right after it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdMark(usize, u64);

impl CmdMark {
    pub fn now(vp: &Viewport) -> CmdMark {
        CmdMark(vp.history.undo_len(), vp.scene.version)
    }

    /// A mark no command matches (before one ran).
    pub fn sentinel() -> CmdMark {
        CmdMark(usize::MAX, u64::MAX)
    }

    /// The marked command is still the last thing that happened.
    pub fn is_last(&self, vp: &Viewport) -> bool {
        *self == CmdMark::now(vp)
    }
}

/// A face copied by `m`.
fn mapped_face(f: &FaceSnap, m: &DMat4) -> Box<dyn Command> {
    let attrs = (!f.attrs.is_empty()).then(|| f.attrs.clone());
    Box::new(AddFaceCommand::plain(
        f.outer.iter().map(|p| m.project_point(*p)).collect(),
        f.holes
            .iter()
            .map(|h| h.iter().map(|p| m.project_point(*p)).collect())
            .collect(),
        attrs,
    ))
}

/// Copies stamped by translation (Move): groups, faces, then edges —
/// plain edges through the edge planner (they split what they cross),
/// curves and soft edges with their flags.
pub struct TranslateCopies {
    pub groups: Vec<GroupId>,
    pub faces: Vec<FaceSnap>,
    pub edges: Vec<EdgeSnap>,
    pub deltas: Vec<DVec3>,
}

impl Command for TranslateCopies {
    fn name(&self) -> &str {
        "CompoundCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let k = |p: DVec3| {
            let r = |x: f64| (x * 1e6).round() as i64;
            (r(p.x), r(p.y), r(p.z))
        };
        let mut face_sides = HashSet::new();
        for f in &self.faces {
            for lp in std::iter::once(&f.outer).chain(f.holes.iter()) {
                let n = lp.len();
                for i in 0..n {
                    let (a, b) = (k(lp[i]), k(lp[(i + 1) % n]));
                    face_sides.insert(if a <= b { (a, b) } else { (b, a) });
                }
            }
        }
        let bounds_a_face = |e: &EdgeSnap| {
            let (a, b) = (k(e.a), k(e.b));
            face_sides.contains(&if a <= b { (a, b) } else { (b, a) })
        };
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        let mut plain: Vec<(DVec3, DVec3)> = Vec::new();
        for &d in &self.deltas {
            let m = DMat4::from_translation(d);
            for &gid in &self.groups {
                let Some(mut g) = scene.doc.find_group(gid).cloned() else {
                    continue;
                };
                let copy = copy_group(&mut scene.doc.meshes, &mut g, Some(d), false);
                if let Some(orig) = scene.doc.find_group_mut(gid) {
                    *orig = g;
                }
                cmds.push(Box::new(InsertGroupCommand::new(copy)));
            }
            for f in &self.faces {
                cmds.push(mapped_face(f, &m));
            }
            let mut id_map: Vec<(i64, i64)> = Vec::new();
            for e in &self.edges {
                if e.curve.is_none() && !e.soft && !bounds_a_face(e) {
                    plain.push((e.a + d, e.b + d));
                    continue;
                }
                let curve = e.curve.map(|c| match id_map.iter().find(|x| x.0 == c) {
                    Some(x) => x.1,
                    None => {
                        let n = Mesh::next_curve_id();
                        id_map.push((c, n));
                        n
                    }
                });
                cmds.push(Box::new(AddEdgeCommand::with_flags(
                    e.a + d,
                    e.b + d,
                    e.soft.then_some(true),
                    curve,
                )));
            }
        }
        if !plain.is_empty() {
            cmds.push(xtrude_model::edits::build_add_edges(
                scene,
                &plain,
                true,
                Vec::new(),
                None,
            ));
        }
        for c in cmds.iter_mut() {
            c.apply(scene)?;
        }
        Ok(())
    }
}

/// Copies stamped by rotation (Rotate): per angle, each group copied and
/// turned, the faces and edges mapped (one fresh curve id per curve and
/// copy).
pub struct RotateCopies {
    pub groups: Vec<GroupId>,
    pub faces: Vec<FaceSnap>,
    pub edges: Vec<EdgeSnap>,
    pub center: DVec3,
    pub axis: DVec3,
    pub degs: Vec<f64>,
}

impl Command for RotateCopies {
    fn name(&self) -> &str {
        "CompoundCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        let mut id_map: Vec<((i64, u64), i64)> = Vec::new();
        for &deg in &self.degs {
            let m = xtrude_model::commands::rotation_matrix(self.center, self.axis, deg);
            for &gid in &self.groups {
                let Some(mut g) = scene.doc.find_group(gid).cloned() else {
                    continue;
                };
                let copy = copy_group(&mut scene.doc.meshes, &mut g, None, false);
                if let Some(orig) = scene.doc.find_group_mut(gid) {
                    *orig = g;
                }
                let cid = copy.id;
                cmds.push(Box::new(InsertGroupCommand::new(copy)));
                cmds.push(Box::new(RotateGroupCommand {
                    group: cid,
                    center: self.center,
                    axis: self.axis,
                    degrees: deg,
                }));
            }
            for f in &self.faces {
                cmds.push(mapped_face(f, &m));
            }
            for e in &self.edges {
                let curve = e.curve.map(|c| {
                    let key = (c, deg.to_bits());
                    match id_map.iter().find(|x| x.0 == key) {
                        Some(x) => x.1,
                        None => {
                            let n = Mesh::next_curve_id();
                            id_map.push((key, n));
                            n
                        }
                    }
                });
                cmds.push(Box::new(AddEdgeCommand::with_flags(
                    m.project_point(e.a),
                    m.project_point(e.b),
                    e.soft.then_some(true),
                    curve,
                )));
            }
        }
        for c in cmds.iter_mut() {
            c.apply(scene)?;
        }
        Ok(())
    }
}

/// Copies stamped by a mirror (Flip): each group copied and flipped, the
/// faces mirrored and re-reversed so they face out, the edges mirrored.
pub struct FlipCopies {
    pub groups: Vec<GroupId>,
    pub faces: Vec<FaceSnap>,
    pub edges: Vec<EdgeSnap>,
    pub centre: DVec3,
    pub axis: DVec3,
}

impl Command for FlipCopies {
    fn name(&self) -> &str {
        "CompoundCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let m = xtrude_model::commands::mirror_matrix(self.centre, self.axis);
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        for &gid in &self.groups {
            let Some(mut g) = scene.doc.find_group(gid).cloned() else {
                continue;
            };
            let copy = copy_group(&mut scene.doc.meshes, &mut g, None, false);
            if let Some(orig) = scene.doc.find_group_mut(gid) {
                *orig = g;
            }
            let cid = copy.id;
            cmds.push(Box::new(InsertGroupCommand::new(copy)));
            cmds.push(Box::new(xtrude_model::commands::FlipGroupsCommand {
                groups: vec![cid],
                centre: self.centre,
                axis: self.axis,
            }));
        }
        for f in &self.faces {
            let lp: Vec<DVec3> = f.outer.iter().rev().map(|p| m.project_point(*p)).collect();
            let holes: Vec<Vec<DVec3>> = f
                .holes
                .iter()
                .map(|h| h.iter().rev().map(|p| m.project_point(*p)).collect())
                .collect();
            let attrs = (!f.attrs.is_empty()).then(|| f.attrs.clone());
            cmds.push(Box::new(AddFaceCommand::plain(lp, holes, attrs)));
        }
        let mut id_map: Vec<(i64, i64)> = Vec::new();
        for e in &self.edges {
            let curve = e.curve.map(|c| match id_map.iter().find(|x| x.0 == c) {
                Some(x) => x.1,
                None => {
                    let n = Mesh::next_curve_id();
                    id_map.push((c, n));
                    n
                }
            });
            cmds.push(Box::new(AddEdgeCommand::with_flags(
                m.project_point(e.a),
                m.project_point(e.b),
                e.soft.then_some(true),
                curve,
            )));
        }
        for c in cmds.iter_mut() {
            c.apply(scene)?;
        }
        Ok(())
    }
}
