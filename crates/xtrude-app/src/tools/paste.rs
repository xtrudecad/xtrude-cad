// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Paste tool: drop a copied set of geometry, placing it with the cursor.
//!
//! The copied faces, edges and groups follow the cursor (snapping like any
//! draw), a click stamps them ONCE as one undo step and hands back to
//! Select. The clipboard is kept — paste again for another copy. Undo
//! restores the document snapshot, vertices included.

use std::collections::HashMap;

use glam::DVec3;
use xtrude_core::geometry::PolyFace;
use xtrude_core::math::newell;
use xtrude_core::Mesh;
use xtrude_io::clip::ClipData;
use xtrude_model::commands::{
    AddEdgeCommand, AddFaceCommand, InsertGroupCommand, RebuildPlaneFacesCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{copy_group, iter_placements};
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::ids::GroupId;
use xtrude_model::scene::{Scene, SelectMode};

use super::{Rgba, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::{Viewport, ViewportEvent};

/// More distinct planes than this in one paste is 3D geometry being moved
/// about, not a flat drawing laid over another: no merge is attempted.
const MERGE_PLANES_CAP: usize = 24;

#[derive(Default)]
pub struct PasteTool {
    clip: Option<ClipData>,
    offset: DVec3,
    preview_on: bool,
}

impl PasteTool {
    pub fn new() -> PasteTool {
        PasteTool::default()
    }

    /// A tool holding `clip` (Paste in Place, tests).
    pub fn with_clip(clip: ClipData) -> PasteTool {
        PasteTool {
            clip: Some(clip),
            ..PasteTool::default()
        }
    }

    fn end_preview(&mut self, vp: &mut Viewport) {
        if self.preview_on {
            vp.end_groups_preview();
            self.preview_on = false;
        }
    }

    /// Paste in Place: stamp the clipboard at the exact coordinates it was
    /// copied from; everything pasted ends up selected.
    pub fn in_place(vp: &mut Viewport) -> bool {
        let Some(clip) = vp.clipboard.clone() else {
            return false;
        };
        let mut tool = PasteTool::with_clip(clip);
        let ok = tool.stamp(vp, DVec3::ZERO, true);
        vp.update();
        ok
    }

    /// Stamp the clipboard displaced by `off` as ONE undoable step; false
    /// when there was nothing to stamp. `select_all` leaves the loose faces
    /// and edges selected too (groups always are).
    pub fn stamp(&mut self, vp: &mut Viewport, off: DVec3, select_all: bool) -> bool {
        let Some(clip) = self.clip.clone() else {
            return false;
        };
        let mut commands: Vec<Box<dyn Command>> = Vec::new();
        // A face's boundary edges first, as commands of their own, so the
        // stamp owns them.
        for (lp, holes, _) in &clip.faces {
            for ring in std::iter::once(lp).chain(holes.iter()) {
                let n = ring.len();
                for i in 0..n {
                    commands.push(Box::new(AddEdgeCommand::new(
                        ring[i] + off,
                        ring[(i + 1) % n] + off,
                    )));
                }
            }
        }
        for (lp, holes, attrs) in &clip.faces {
            // Copied attrs travel onto the pasted face.
            let attrs = (!attrs.is_empty()).then(|| attrs.clone());
            commands.push(Box::new(AddFaceCommand::plain(
                lp.iter().map(|p| *p + off).collect(),
                holes
                    .iter()
                    .map(|h| h.iter().map(|p| *p + off).collect())
                    .collect(),
                attrs,
            )));
        }
        // A face pasted onto a plane that already carries faces merges
        // with them.
        let loops: Vec<Vec<DVec3>> = clip
            .faces
            .iter()
            .map(|(lp, _, _)| lp.iter().map(|p| *p + off).collect())
            .collect();
        for c in plane_merges(&vp.scene, &loops) {
            commands.push(Box::new(c));
        }
        // Curve ids are remapped to FRESH ones so each pasted circle/arc
        // is its own contour.
        let mut id_map: HashMap<i64, i64> = HashMap::new();
        for &(a, b, soft, curve) in &clip.edges {
            let curve = curve.map(|c| *id_map.entry(c).or_insert_with(Mesh::next_curve_id));
            commands.push(Box::new(AddEdgeCommand::with_flags(
                a + off,
                b + off,
                soft.then_some(true),
                curve,
            )));
        }
        // Each stamp builds FRESH group copies from the templates
        // (instances stay siblings of the same prototype).
        let mut pasted: Vec<GroupId> = Vec::new();
        for tpl in &clip.groups {
            for (pg, _) in iter_placements(tpl, None) {
                if let Some(m) = clip.meshes.get(&pg.mesh) {
                    vp.scene
                        .doc
                        .meshes
                        .entry(pg.mesh)
                        .or_insert_with(|| m.clone());
                }
            }
            let mut t = tpl.clone();
            let g = copy_group(&mut vp.scene.doc.meshes, &mut t, Some(off), false);
            pasted.push(g.id);
            commands.push(Box::new(InsertGroupCommand::new(g)));
        }
        if commands.is_empty() {
            return false; // nothing to stamp
        }
        // The scratch preview ends BEFORE the stamp.
        self.end_preview(vp);
        vp.execute(&mut CompoundCommand::new(commands));
        let mut picked: Vec<EntityRef> = pasted.into_iter().map(EntityRef::Group).collect();
        if select_all {
            let mid = vp.scene.active_mesh_id();
            let mesh = vp.scene.mesh();
            // The pasted faces, looked up by their outline (a merge may
            // have rebuilt them; one that no longer exists is skipped).
            let key = |p: DVec3| {
                [
                    (p.x * 1e6).round() as i64,
                    (p.y * 1e6).round() as i64,
                    (p.z * 1e6).round() as i64,
                ]
            };
            for lp in &loops {
                let mut want: Vec<[i64; 3]> = lp.iter().map(|p| key(*p)).collect();
                want.sort();
                let hit = mesh.face_ids().into_iter().find(|&f| {
                    let mut have: Vec<[i64; 3]> =
                        mesh.face_positions(f).iter().map(|p| key(*p)).collect();
                    have.sort();
                    have == want
                });
                if let Some(f) = hit {
                    picked.push(EntityRef::Face(mid, f));
                }
            }
            // The copied edges themselves, looked up by position.
            for &(a, b, _, _) in &clip.edges {
                let (v0, v1) = (mesh.vertex_at(a + off), mesh.vertex_at(b + off));
                if let (Some(v0), Some(v1)) = (v0, v1) {
                    if let Some(e) = mesh.find_edge(v0, v1) {
                        picked.push(EntityRef::Edge(mid, e));
                    }
                }
            }
        }
        if !picked.is_empty() {
            vp.scene.select(picked, SelectMode::Replace);
        }
        true
    }
}

/// Scoped plane rebuilds for the planes of the pasted `loops` that already
/// carry faces of the target mesh.
fn plane_merges(scene: &Scene, loops: &[Vec<DVec3>]) -> Vec<RebuildPlaneFacesCommand> {
    struct Plane {
        origin: DVec3,
        n: DVec3,
        lo: DVec3,
        hi: DVec3,
    }
    let mut planes: Vec<((i64, i64, i64, i64), Plane)> = Vec::new();
    let r4 = |x: f64| (x * 1e4).round() as i64;
    for lp in loops {
        if lp.len() < 3 {
            continue;
        }
        let n = newell(lp);
        let len = n.length();
        if len < 1e-9 {
            continue;
        }
        let mut n = n / len;
        if n.z < 0.0 || (n.z == 0.0 && (n.y < 0.0 || (n.y == 0.0 && n.x < 0.0))) {
            n = -n;
        }
        let d = n.dot(lp[0]);
        let key = (r4(n.x), r4(n.y), r4(n.z), r4(d));
        let idx = match planes.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => {
                planes.push((
                    key,
                    Plane {
                        origin: lp[0],
                        n,
                        lo: DVec3::splat(f64::INFINITY),
                        hi: DVec3::splat(f64::NEG_INFINITY),
                    },
                ));
                planes.len() - 1
            }
        };
        for p in lp {
            planes[idx].1.lo = planes[idx].1.lo.min(*p);
            planes[idx].1.hi = planes[idx].1.hi.max(*p);
        }
        if planes.len() > MERGE_PLANES_CAP {
            return Vec::new();
        }
    }
    let mesh = scene.mesh();
    if planes.is_empty() || mesh.faces.is_empty() {
        return Vec::new();
    }
    let pts: Vec<DVec3> = mesh.verts.iter().map(|(_, v)| v.position).collect();
    let tol = 1e-4;
    let mut out = Vec::new();
    for (_, pl) in &planes {
        let d = pl.n.dot(pl.origin);
        if pts.iter().filter(|p| (p.dot(pl.n) - d).abs() < tol).count() < 3 {
            continue;
        }
        for f in mesh.face_ids() {
            let vs = mesh.face_positions(f);
            if vs.iter().any(|v| (*v - pl.origin).dot(pl.n).abs() > tol) {
                continue;
            }
            let flo = vs
                .iter()
                .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
            let fhi = vs
                .iter()
                .fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
            let overlaps = (0..3).all(|i| {
                (flo[i] < pl.hi[i] - tol && pl.lo[i] < fhi[i] - tol)
                    || ((fhi[i] - flo[i]).abs() < tol && (pl.hi[i] - pl.lo[i]).abs() < tol)
            });
            if overlaps {
                out.push(RebuildPlaneFacesCommand::new(pl.origin, pl.n));
                break;
            }
        }
    }
    out
}

impl Tool for PasteTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo::new("paste", "Paste");
        &INFO
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    /// The clipboard's reference point, as the viewport's "start": the
    /// cursor lands on the plane THROUGH it.
    fn start_point(&self) -> Option<DVec3> {
        self.clip.as_ref().map(|c| c.reference)
    }

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.clip = vp.clipboard.clone();
        self.offset = DVec3::ZERO;
        self.preview_on = false;
        let groups: Vec<GroupId> = self
            .clip
            .as_ref()
            .map(|c| c.groups.iter().map(|g| g.id).collect())
            .unwrap_or_default();
        if !groups.is_empty() {
            // Copied groups preview through the scratch pipeline: the full
            // model follows the cursor at zero per-frame cost.
            vp.begin_external_groups_preview(&groups);
            self.preview_on = true;
        }
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.end_preview(vp);
        self.clip = None;
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(clip) = &self.clip else {
            return;
        };
        self.offset = ctx.world - clip.reference;
        if self.preview_on {
            vp.set_groups_preview_offset(self.offset);
        } else {
            vp.update();
        }
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(reference) = self.clip.as_ref().map(|c| c.reference) else {
            return;
        };
        if !self.stamp(vp, ctx.world - reference, false) {
            return;
        }
        // One stamp per paste: hand back to Select (the clipboard stays).
        vp.outbox
            .events
            .push(ViewportEvent::ActivateTool("select".into()));
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.end_preview(vp);
        self.clip = None;
        vp.update();
    }

    /// Only the LOOSE faces/edges of the clipboard go through here; groups
    /// preview through the viewport's scratch pipeline.
    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(clip) = &self.clip else {
            return Vec::new();
        };
        let off = self.offset;
        let mut segs = Vec::new();
        for (lp, holes, _) in &clip.faces {
            for ring in std::iter::once(lp).chain(holes.iter()) {
                let n = ring.len();
                for i in 0..n {
                    segs.push((ring[i] + off, ring[(i + 1) % n] + off));
                }
            }
        }
        for &(a, b, _, _) in &clip.edges {
            segs.push((a + off, b + off));
        }
        segs
    }

    fn preview_faces(&self) -> Vec<PolyFace> {
        let Some(clip) = &self.clip else {
            return Vec::new();
        };
        let off = self.offset;
        clip.faces
            .iter()
            .map(|(lp, holes, attrs)| PolyFace {
                vertices: lp.iter().map(|p| *p + off).collect(),
                holes: holes
                    .iter()
                    .map(|h| h.iter().map(|p| *p + off).collect())
                    .collect(),
                attrs: (!attrs.is_empty()).then(|| attrs.clone()),
            })
            .collect()
    }
}
