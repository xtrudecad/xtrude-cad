// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Sweep tool: sweep a profile face along a path — dragged along the edges
//! (highlighted red, Alt over a face takes its perimeter), preselected as a
//! chain of edges, or a preselected face's boundary. The sweep itself is
//! `xtrude_core::sweep`.

use glam::DVec3;
use xtrude_core::math::VecExt;
use xtrude_core::sweep::{
    manual_path_extend, manual_path_start, order_path_edges, orient_closed_path,
    sweep_preview_faces, sweep_profile,
};
use xtrude_core::{EdgeId, FaceId, VertexId};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::i18n::tr;
use xtrude_model::ids::MeshId;

use super::{PreviewFace, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::Viewport;

/// Below this many pixels between press and release, a drag was a click.
const DRAG_PX: f64 = 6.0;

#[derive(Default)]
pub struct SweepTool {
    pub hovered_face: Option<FaceId>,
    /// A preselected path and whether it closes.
    pub path: Option<Vec<DVec3>>,
    pub closed: bool,
    pub profile: Option<FaceId>,
    pub chain: Vec<VertexId>,
    pub start_pt: Option<DVec3>,
    pub alt_face: Option<FaceId>,
    preview: Vec<PreviewFace>,
    preview_pts: Vec<DVec3>,
    preview_closed: bool,
    press_screen: Option<(f64, f64)>,
    last_screen: Option<(f64, f64)>,
}

impl SweepTool {
    pub fn new() -> SweepTool {
        SweepTool::default()
    }

    fn reset_drag(&mut self) {
        self.profile = None;
        self.chain.clear();
        self.start_pt = None;
        self.alt_face = None;
        self.preview.clear();
        self.preview_pts.clear();
        self.preview_closed = false;
        self.press_screen = None;
        self.last_screen = None;
    }

    /// Edges in the profile's plane are never path edges.
    fn in_profile_plane(&self, vp: &Viewport, edge: EdgeId) -> bool {
        let Some(face) = self.profile else {
            return false;
        };
        let m = vp.scene.mesh();
        if !m.has_face(face) || !m.has_edge(edge) {
            return false;
        }
        let c = m.face_centroid(face);
        let n = m.face_normal(face).unit_or_zero();
        let (a, b) = m.edge_ab(edge);
        (a - c).dot(n).abs() < 1e-6 && (b - c).dot(n).abs() < 1e-6
    }

    /// The dragged path as points, and whether it closes.
    pub fn path_points(&self, vp: &Viewport) -> (Vec<DVec3>, bool) {
        let m = vp.scene.mesh();
        if let (Some(alt), Some(profile)) = (self.alt_face, self.profile) {
            return (
                orient_closed_path(
                    &m.face_positions(alt),
                    m.face_centroid(profile),
                    m.face_normal(profile),
                ),
                true,
            );
        }
        let mut pts: Vec<DVec3> = self.chain.iter().map(|&v| m.pos(v)).collect();
        let closed = self.chain.len() >= 4
            && self.chain.last() == self.chain.first()
            && self.start_pt.is_none();
        if closed {
            pts.pop();
        } else if let Some(s) = self.start_pt {
            pts.insert(0, s);
        }
        (pts, closed)
    }

    fn rebuild_preview(&mut self, vp: &Viewport) {
        let (pts, closed) = self.path_points(vp);
        self.preview = match self.profile {
            Some(f) if pts.len() >= 2 => {
                sweep_preview_faces(&vp.scene.mesh().poly_face(f), &pts, closed)
            }
            _ => Vec::new(),
        };
        self.preview_pts = pts;
        self.preview_closed = closed;
    }

    fn finish(&mut self, vp: &mut Viewport) {
        let face = self.profile;
        let (pts, closed) = self.path_points(vp);
        self.reset_drag();
        let Some(face) = face.filter(|_| pts.len() >= 2) else {
            vp.flash_status(
                &tr("Sweep: touch the path edges to extrude along them", &[]),
                2500,
            );
            vp.update();
            return;
        };
        self.commit(vp, face, pts, closed);
    }

    fn commit(&mut self, vp: &mut Viewport, face: FaceId, path: Vec<DVec3>, closed: bool) {
        let ok = std::cell::Cell::new(false);
        vp.set_hover(None);
        self.hovered_face = None;
        vp.run(FnCommand::new(
            "SnapshotMutation",
            |s: &mut xtrude_model::scene::Scene| {
                ok.set(sweep_profile(s.mesh_mut(), face, &path, closed));
                Ok(())
            },
        ));
        if !ok.get() {
            vp.history.discard_last(&mut vp.scene);
            vp.flash_status(
                &tr(
                    "Sweep: could not sweep along that path (reversal or degenerate segment)",
                    &[],
                ),
                2500,
            );
        } else {
            vp.scene.selection.clear();
        }
        vp.update();
    }

    fn orient_path(&self, vp: &Viewport, face: FaceId) -> (Vec<DVec3>, bool) {
        let m = vp.scene.mesh();
        let mut path = self.path.clone().unwrap_or_default();
        let c = m.face_centroid(face);
        if self.closed {
            return (orient_closed_path(&path, c, m.face_normal(face)), true);
        }
        if let (Some(first), Some(last)) = (path.first(), path.last()) {
            if (*last - c).length() < (*first - c).length() {
                path.reverse();
            }
        }
        (path, false)
    }
}

impl Tool for SweepTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("sweep"),
            shortcut: Some("W"),
            description: Some(
                "Sweep a face along a path of edges — mouldings, pipes, turned shapes.",
            ),
            ..ToolInfo::new("sweep", "Sweep")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.85, 0.16, 0.16, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.hovered_face = None;
        self.path = None;
        self.closed = false;
        self.reset_drag();
        let mid = vp.scene.active_mesh_id();
        let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
        let edges: Vec<EdgeId> = sel
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Edge(m, id) if m == mid => Some(id),
                _ => None,
            })
            .collect();
        let faces: Vec<FaceId> = sel
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Face(m, id) if m == mid => Some(id),
                _ => None,
            })
            .collect();
        if !edges.is_empty() {
            match order_path_edges(vp.scene.mesh(), &edges) {
                None => {
                    vp.flash_status(
                        &tr("Sweep: the selected edges must form one simple path", &[]),
                        2500,
                    );
                }
                Some((p, c)) => {
                    self.path = Some(p);
                    self.closed = c;
                }
            }
        } else if faces.len() == 1 {
            self.path = Some(vp.scene.mesh().face_positions(faces[0]));
            self.closed = true;
        } else {
            vp.flash_status(
                &tr(
                    "Sweep: click the profile and drag along the path (it highlights red), click to finish — or select the path first, then click the profile",
                    &[],
                ),
                6000,
            );
        }
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
        self.hovered_face = None;
        self.reset_drag();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (x, y) = ctx.screen;
        let Some(profile) = self.profile else {
            self.hovered_face = vp.pick_face(x, y).map(|f| f.1);
            vp.set_hover(self.hovered_face.map(|f| EntityRef::Face(MeshId::LOOSE, f)));
            return;
        };
        self.last_screen = Some((x, y));
        let mut changed = false;
        if ctx.modifiers.alt {
            if let Some((_, f)) = vp.pick_face(x, y) {
                if f != profile && Some(f) != self.alt_face {
                    self.alt_face = Some(f);
                    changed = true;
                }
            }
        } else {
            if self.alt_face.is_some() {
                self.alt_face = None;
                changed = true;
            }
            if let Some((_, e)) = vp.pick_edge(x, y).and_then(|e| e.edge) {
                if !self.in_profile_plane(vp, e) {
                    if self.chain.is_empty() {
                        let (s, ch) =
                            manual_path_start(vp.scene.mesh(), profile, e, Some(ctx.world));
                        self.start_pt = s;
                        self.chain = ch;
                        changed = true;
                    } else {
                        let mut chain = std::mem::take(&mut self.chain);
                        let skip = |ed: EdgeId| self.in_profile_plane(vp, ed);
                        changed = manual_path_extend(vp.scene.mesh(), &mut chain, e, &skip, 400);
                        self.chain = chain;
                    }
                }
            }
        }
        if changed {
            self.rebuild_preview(vp);
            vp.update();
        }
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.profile.is_some() {
            self.finish(vp);
            return;
        }
        let Some(face) = self.hovered_face else {
            return;
        };
        if self.path.is_some() {
            let (path, closed) = self.orient_path(vp, face);
            self.commit(vp, face, path, closed);
            return;
        }
        self.profile = Some(face);
        self.chain.clear();
        self.start_pt = None;
        self.alt_face = None;
        self.preview.clear();
        self.preview_pts.clear();
        self.press_screen = Some(ctx.screen);
        self.last_screen = Some(ctx.screen);
        vp.set_hover(None);
        vp.flash_status(
            &tr(
                "Sweep: drag along the path (it highlights red) — click to finish, Esc to cancel; hold Alt over a face to follow its perimeter",
                &[],
            ),
            6000,
        );
        vp.update();
    }

    fn on_release(&mut self, vp: &mut Viewport) {
        if self.profile.is_none() || self.preview_pts.is_empty() {
            return;
        }
        let (Some(a), Some(b)) = (self.press_screen, self.last_screen) else {
            return;
        };
        if (a.0 - b.0).hypot(a.1 - b.1) >= DRAG_PX {
            self.finish(vp);
        }
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset_drag();
        vp.set_hover(None);
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let pts = &self.preview_pts;
        if pts.len() < 2 {
            return Vec::new();
        }
        let mut segs: Vec<(DVec3, DVec3)> = pts.windows(2).map(|w| (w[0], w[1])).collect();
        if self.preview_closed {
            segs.push((pts[pts.len() - 1], pts[0]));
        }
        segs
    }

    fn preview_faces(&self) -> Vec<PreviewFace> {
        self.preview.clone()
    }

    fn busy(&self) -> bool {
        false
    }

    fn busy_for_delete(&self) -> bool {
        self.profile.is_some()
    }
}
