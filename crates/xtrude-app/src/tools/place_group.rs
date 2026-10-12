// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Place-component tool: a freshly built group follows the cursor and a click
//! drops it.
//!
//! The group hangs from the CENTRE OF ITS BASE (or a point the caller
//! names, as an imported document hangs from its origin), so it tries to
//! sit on whatever the cursor hovers. With face alignment (3D text) a
//! hovered face turns the group's front (-Y) along the face normal. Esc
//! discards the pending component. The group's mesh already lives in the
//! document's mesh table; only the group enters the scene on the click.

use glam::{DMat4, DVec3, DVec4};
use xtrude_core::math::VecExt;
use xtrude_core::snap::face_plane_world;
use xtrude_model::commands::InsertGroupCommand;
use xtrude_model::group::{carry_axes, iter_placements, placement_points, Group, MeshTable};
use xtrude_model::i18n::tr;

use super::{Rgba, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::{Viewport, ViewportEvent};

/// Preview segments are plenty for the bundled starters; a hard cap keeps
/// a huge component from turning the hover into a slideshow.
const MAX_PREVIEW_EDGES: usize = 2000;

pub struct PlaceGroupTool {
    group: Option<Group>,
    /// A component INSTANCE is placed by composing its matrix; a classic
    /// group by moving its vertices.
    instance: bool,
    /// What the cursor holds.
    pub anchor: DVec3,
    offset: DVec3,
    align: bool,
    face_normal: Option<DVec3>,
    /// Local preview segments, relative to the anchor.
    segments: Vec<(DVec3, DVec3)>,
}

impl PlaceGroupTool {
    /// `group`'s mesh (and its children's) must be in `meshes` already.
    pub fn new(
        meshes: &MeshTable,
        group: Group,
        align_to_face: bool,
        anchor: Option<DVec3>,
    ) -> PlaceGroupTool {
        let instance = group.xform.is_some();
        let anchor = anchor.unwrap_or_else(|| Self::base_center(meshes, &group));
        let mut segments = if !group.children.is_empty() {
            Self::placement_segments(meshes, &group, anchor)
        } else {
            meshes
                .get(&group.mesh)
                .map(|m| {
                    m.edge_ids()
                        .into_iter()
                        .take(MAX_PREVIEW_EDGES)
                        .map(|e| {
                            let (a, b) = m.edge_ab(e);
                            (a - anchor, b - anchor)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        if segments.is_empty() {
            segments = Self::bbox_segments(meshes, &group, anchor);
        }
        PlaceGroupTool {
            group: Some(group),
            instance,
            anchor,
            offset: DVec3::ZERO,
            align: align_to_face,
            face_normal: None,
            segments,
        }
    }

    /// How many preview segments the cursor carries.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// The group still waiting to be placed.
    pub fn pending(&self) -> Option<&Group> {
        self.group.as_ref()
    }

    /// Edges of the group and its nested placements in world space,
    /// relative to the anchor — empty past the preview cap.
    fn placement_segments(meshes: &MeshTable, group: &Group, anchor: DVec3) -> Vec<(DVec3, DVec3)> {
        let placements = iter_placements(group, None);
        let total: usize = placements
            .iter()
            .map(|(g, _)| meshes.get(&g.mesh).map_or(0, |m| m.edges.len()))
            .sum();
        if total > MAX_PREVIEW_EDGES {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (g, m) in placements {
            let Some(mesh) = meshes.get(&g.mesh) else {
                continue;
            };
            for e in mesh.edge_ids() {
                let (mut a, mut b) = mesh.edge_ab(e);
                if let Some(m) = m {
                    a = m.transform_point3(a);
                    b = m.transform_point3(b);
                }
                out.push((a - anchor, b - anchor));
            }
        }
        out
    }

    /// Wireframe box fallback: meshes without edges and containers.
    fn bbox_segments(meshes: &MeshTable, group: &Group, anchor: DVec3) -> Vec<(DVec3, DVec3)> {
        let pts = placement_points(meshes, group);
        if pts.is_empty() {
            return Vec::new();
        }
        let lo = pts
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p))
            - anchor;
        let hi = pts
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p))
            - anchor;
        let mut c = Vec::with_capacity(8);
        for z in [lo.z, hi.z] {
            for y in [lo.y, hi.y] {
                for x in [lo.x, hi.x] {
                    c.push(DVec3::new(x, y, z));
                }
            }
        }
        const IDX: [(usize, usize); 12] = [
            (0, 1),
            (1, 3),
            (3, 2),
            (2, 0),
            (4, 5),
            (5, 7),
            (7, 6),
            (6, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        IDX.iter().map(|&(i, j)| (c[i], c[j])).collect()
    }

    pub fn base_center(meshes: &MeshTable, group: &Group) -> DVec3 {
        let pts = placement_points(meshes, group);
        if pts.is_empty() {
            return DVec3::ZERO;
        }
        let lo = pts
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
        let hi = pts
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z)
    }

    /// Orthonormal frame turning the group's local axes onto a face: local
    /// -Y (its front) → the face normal, the text's up world-up on vertical faces
    /// and world-north on a horizontal face. `(right, y_axis, up)` = the
    /// images of local +X, +Y, +Z.
    pub fn face_frame(normal: DVec3) -> (DVec3, DVec3, DVec3) {
        let n = normal.unit_or_zero();
        let hint = if n.z.abs() > 0.99 { DVec3::Y } else { DVec3::Z };
        let mut right = hint.cross(n);
        if right.length() < 1e-9 {
            right = DVec3::X;
        }
        let right = right.unit_or_zero();
        let up = n.cross(right).unit_or_zero();
        (right, -n, up)
    }

    fn rotate(&self, p: DVec3) -> DVec3 {
        match self.face_normal {
            None => p,
            Some(n) => {
                let (r, y, u) = Self::face_frame(n);
                r * p.x + y * p.y + u * p.z
            }
        }
    }

    /// The move just applied, as one matrix: the alignment turn, then
    /// `shift`.
    fn pose_matrix(&self, shift: DVec3) -> DMat4 {
        let t = DMat4::from_translation(shift);
        match self.face_normal {
            None => t,
            Some(n) => {
                let (r, y, u) = Self::face_frame(n);
                t * DMat4::from_cols(
                    r.extend(0.0),
                    y.extend(0.0),
                    u.extend(0.0),
                    DVec4::new(0.0, 0.0, 0.0, 1.0),
                )
            }
        }
    }

    fn update_alignment(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if !self.align {
            return;
        }
        self.face_normal = vp
            .pick_face_any(ctx.screen.0, ctx.screen.1)
            .and_then(|hit| {
                let mesh = vp.scene.doc.mesh(hit.mesh);
                mesh.has_face(hit.face)
                    .then(|| face_plane_world(mesh, hit.face, hit.xform.as_ref()).1)
            });
    }

    /// Drop the component with its anchor at `world`, in one undo step —
    /// what a click does, for a script that already knows the point.
    pub fn place_at(&mut self, vp: &mut Viewport, world: DVec3) {
        let Some(mut group) = self.group.take() else {
            return;
        };
        let shift = world - self.rotate(self.anchor);
        let pose = self.pose_matrix(shift);
        if self.instance {
            // The pose composes into the matrix: the prototype stays put.
            group.xform = Some(pose * group.xform.unwrap_or(DMat4::IDENTITY));
        } else {
            // Re-pose the isolated mesh BEFORE it enters the scene (undo of
            // the insert removes the whole group).
            let mesh = vp.scene.doc.mesh_mut(group.mesh);
            for vid in mesh.vertex_ids() {
                let p = mesh.verts[vid].position;
                let delta = self.rotate(p) + shift - p;
                if delta.length() > 1e-9 {
                    mesh.move_vertex(vid, delta);
                }
            }
            carry_axes(&mut group, &pose);
        }
        vp.execute(&mut InsertGroupCommand::new(group));
        vp.flash_status(&tr("Component placed — Move (M) adjusts it", &[]), 2500);
        vp.outbox
            .events
            .push(ViewportEvent::ActivateTool("select".into()));
        vp.update();
    }
}

impl Tool for PlaceGroupTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            wireframe_depth_tested: false,
            ..ToolInfo::new("place_group", "Place component")
        };
        &INFO
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.group = None;
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.group.is_none() {
            return;
        }
        self.update_alignment(vp, ctx);
        self.offset = ctx.world - self.rotate(self.anchor);
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.group.is_none() {
            return;
        }
        self.update_alignment(vp, ctx);
        self.place_at(vp, ctx.world);
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.group = None;
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        if self.group.is_none() {
            return Vec::new();
        }
        let off = self.offset;
        self.segments
            .iter()
            .map(|&(a, b)| {
                (
                    self.rotate(a + self.anchor) + off,
                    self.rotate(b + self.anchor) + off,
                )
            })
            .collect()
    }
}
