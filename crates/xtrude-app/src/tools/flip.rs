// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Flip tool: three translucent axis planes over the selection; one click
//! mirrors it about the plane under the cursor (arrows pick a plane, Ctrl
//! leaves a copy).

use crate::input::ScreenPt;
use glam::DVec3;
use xtrude_core::axes;
use xtrude_core::math::MatExt;
use xtrude_core::Axis;
use xtrude_model::commands::{FlipGroupsCommand, FlipVerticesCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;

use super::transform::{face_snap, EdgeSnap, FaceSnap, FlipCopies};
use super::{OverlayLayer, SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::overlay::{qc, Paint, Pen, Pt};
use crate::viewport::Viewport;

/// The planes' colours (red, green, blue).
pub const PLANE_RGBA: [(Axis, [u8; 3]); 3] = [
    (Axis::X, [216, 56, 68]),
    (Axis::Y, [40, 158, 90]),
    (Axis::Z, [52, 102, 198]),
];

/// What a flip acts on.
pub struct Targets {
    pub groups: Vec<GroupId>,
    pub positions: Vec<DVec3>,
    pub faces: Vec<xtrude_core::FaceId>,
    pub face_snaps: Vec<FaceSnap>,
    pub edges: Vec<EdgeSnap>,
    pub centre: DVec3,
}

fn point_in_poly(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[derive(Default)]
pub struct FlipTool {
    pub hover_point: Option<DVec3>,
    pub hover_axis: Option<Axis>,
    pub lock_axis: Option<Axis>,
    pub copy: bool,
}

impl FlipTool {
    pub fn new() -> FlipTool {
        FlipTool::default()
    }

    fn bounds(vp: &Viewport) -> Option<(DVec3, DVec3)> {
        let mut pts: Vec<DVec3> = Vec::new();
        for e in &vp.scene.selection {
            match *e {
                EntityRef::Group(g) => {
                    let Some(g) = vp.scene.doc.find_group(g) else {
                        continue;
                    };
                    let m = vp.scene.doc.mesh(g.mesh);
                    pts.extend(
                        m.verts.iter().map(|(_, v)| {
                            g.xform.map_or(v.position, |x| x.project_point(v.position))
                        }),
                    );
                }
                EntityRef::Face(m, f) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_face(f) {
                        pts.extend(mesh.face_positions(f));
                    }
                }
                EntityRef::Edge(m, e) => {
                    let mesh = vp.scene.doc.mesh(m);
                    if mesh.has_edge(e) {
                        let (a, b) = mesh.edge_ab(e);
                        pts.extend([a, b]);
                    }
                }
                _ => {}
            }
        }
        if pts.is_empty() {
            return None;
        }
        let lo = pts.iter().fold(DVec3::splat(f64::MAX), |a, p| a.min(*p));
        let hi = pts.iter().fold(DVec3::splat(f64::MIN), |a, p| a.max(*p));
        Some((lo, hi))
    }

    /// The selection to flip, `None` when nothing is selected.
    pub fn targets(vp: &Viewport) -> Option<Targets> {
        let mid = vp.scene.active_mesh_id();
        let mesh = vp.scene.mesh();
        let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
        let groups: Vec<GroupId> = sel.iter().filter_map(|e| e.as_group()).collect();
        let faces: Vec<xtrude_core::FaceId> = sel
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Face(m, f) if m == mid && mesh.has_face(f) => Some(f),
                _ => None,
            })
            .collect();
        let edges: Vec<xtrude_core::EdgeId> = sel
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Edge(m, id) if m == mid && mesh.has_edge(id) => Some(id),
                _ => None,
            })
            .collect();
        if groups.is_empty() && faces.is_empty() && edges.is_empty() {
            return None;
        }
        let mut positions = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let key = |p: DVec3| {
            let r = |x: f64| (x * 1e6).round() as i64;
            (r(p.x), r(p.y), r(p.z))
        };
        for &f in &faces {
            for p in mesh.face_positions(f) {
                if seen.insert(key(p)) {
                    positions.push(p);
                }
            }
        }
        for &e in &edges {
            let (a, b) = mesh.edge_ab(e);
            for p in [a, b] {
                if seen.insert(key(p)) {
                    positions.push(p);
                }
            }
        }
        let centre = Self::bounds(vp).map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        Some(Targets {
            groups,
            positions,
            face_snaps: faces.iter().map(|&f| face_snap(mesh, f)).collect(),
            edges: edges
                .iter()
                .map(|&e| {
                    let (a, b) = mesh.edge_ab(e);
                    EdgeSnap {
                        a,
                        b,
                        soft: mesh.edge(e).soft,
                        curve: mesh.edge(e).curve,
                    }
                })
                .collect(),
            faces,
            centre,
        })
    }

    /// Mirror the selection across its centre along `axis`, one undo step
    /// (the click, and the context menu's Flip Along).
    pub fn flip(vp: &mut Viewport, axis: Axis, copy: bool) -> bool {
        let Some(t) = Self::targets(vp) else {
            vp.flash_status(&tr("Select the geometry to flip first", &[]), 2500);
            return false;
        };
        let n = axes::axis(axis);
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        if copy {
            cmds.push(Box::new(FlipCopies {
                groups: t.groups,
                faces: t.face_snaps,
                edges: t.edges,
                centre: t.centre,
                axis: n,
            }));
        } else {
            if !t.groups.is_empty() {
                cmds.push(Box::new(FlipGroupsCommand {
                    groups: t.groups,
                    centre: t.centre,
                    axis: n,
                }));
            }
            if !t.positions.is_empty() {
                cmds.push(Box::new(FlipVerticesCommand {
                    src: t.positions,
                    centre: t.centre,
                    axis: n,
                    faces: t.faces,
                }));
            }
        }
        match cmds.len() {
            0 => return false,
            1 => {
                let mut c = cmds.pop().expect("one");
                vp.execute(c.as_mut());
            }
            _ => {
                vp.execute(&mut CompoundCommand::new(cmds));
            }
        }
        vp.update();
        true
    }

    /// The quad of the plane normal to `axis` through `centre`.
    pub fn plane_quad(axis: Axis, c: DVec3, half: DVec3, margin: f64) -> [DVec3; 4] {
        let hx = half.x.max(0.3) * margin;
        let hy = half.y.max(0.3) * margin;
        let hz = half.z.max(0.3) * margin;
        match axis {
            Axis::X => [
                DVec3::new(c.x, c.y - hy, c.z - hz),
                DVec3::new(c.x, c.y + hy, c.z - hz),
                DVec3::new(c.x, c.y + hy, c.z + hz),
                DVec3::new(c.x, c.y - hy, c.z + hz),
            ],
            Axis::Y => [
                DVec3::new(c.x - hx, c.y, c.z - hz),
                DVec3::new(c.x + hx, c.y, c.z - hz),
                DVec3::new(c.x + hx, c.y, c.z + hz),
                DVec3::new(c.x - hx, c.y, c.z + hz),
            ],
            _ => [
                DVec3::new(c.x - hx, c.y - hy, c.z),
                DVec3::new(c.x + hx, c.y - hy, c.z),
                DVec3::new(c.x + hx, c.y + hy, c.z),
                DVec3::new(c.x - hx, c.y + hy, c.z),
            ],
        }
    }

    /// The three planes to draw: `(axis, quad, active)`.
    pub fn planes(&self, vp: &Viewport) -> Vec<(Axis, [DVec3; 4], bool)> {
        let Some((lo, hi)) = Self::bounds(vp) else {
            return Vec::new();
        };
        let (centre, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
        let active = self.lock_axis.or(self.hover_axis);
        Axis::ALL
            .into_iter()
            .map(|a| {
                (
                    a,
                    Self::plane_quad(a, centre, half, 1.15),
                    Some(a) == active,
                )
            })
            .collect()
    }

    fn plane_under_cursor(vp: &Viewport, sx: f64, sy: f64) -> Option<Axis> {
        let (lo, hi) = Self::bounds(vp)?;
        let (centre, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for a in Axis::ALL {
            let quad = Self::plane_quad(a, centre, half, 1.15);
            let pts: Option<Vec<(f64, f64)>> = quad
                .iter()
                .map(|q| vp.world_to_pixel(*q).map(ScreenPt::tuple))
                .collect();
            let Some(pts) = pts else { continue };
            if !point_in_poly((sx, sy), &pts) {
                continue;
            }
            let mid = (
                pts.iter().map(|p| p.0).sum::<f64>() / 4.0,
                pts.iter().map(|p| p.1).sum::<f64>() / 4.0,
            );
            let d = (mid.0 - sx).powi(2) + (mid.1 - sy).powi(2);
            if vp.world_to_pixel(centre).is_some() && d < best_d {
                best_d = d;
                best = Some(a);
            }
        }
        best
    }
}

impl Tool for FlipTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("flip"),
            description: Some("Mirror the selection across its red, green or blue plane."),
            ..ToolInfo::new("flip", "Flip")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    /// The three axis planes through the selection's box.
    fn overlay(&self, vp: &Viewport, layer: OverlayLayer, out: &mut Vec<Paint>) {
        if layer != OverlayLayer::OverSectionPlanes || FlipTool::targets(vp).is_none() {
            return;
        }
        for (axis, quad, active) in self.planes(vp) {
            let pts: Option<Vec<Pt>> = quad
                .iter()
                .map(|q| vp.world_to_pixel(*q).map(ScreenPt::tuple))
                .collect();
            let Some(points) = pts else { continue };
            let Some((_, [r, g, b])) = PLANE_RGBA.iter().find(|(a, _)| *a == axis) else {
                continue;
            };
            out.push(Paint::Polygon {
                points,
                pen: Some(Pen::solid(
                    qc(*r, *g, *b, 255),
                    if active { 2.2 } else { 1.2 },
                )),
                fill: Some(qc(*r, *g, *b, if active { 90 } else { 36 })),
            });
        }
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.hover_axis = None;
        self.lock_axis = None;
        self.copy = false;
        if Self::targets(vp).is_none() {
            vp.flash_status(&tr("Select the geometry to flip first", &[]), 2500);
        }
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.hover_axis = None;
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if key == Key::Control {
            self.copy = !self.copy;
            vp.flash_status(
                &tr(
                    if self.copy {
                        "Flip a copy: on"
                    } else {
                        "Flip a copy: off"
                    },
                    &[],
                ),
                2500,
            );
            vp.update();
            return true;
        }
        let axis = match key {
            Key::Right => Axis::X,
            Key::Left => Axis::Y,
            Key::Up => Axis::Z,
            _ => return false,
        };
        self.lock_axis = if self.lock_axis == Some(axis) {
            None
        } else {
            Some(axis)
        };
        vp.update();
        true
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
        self.hover_axis = Self::plane_under_cursor(vp, ctx.screen.0, ctx.screen.1);
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, _ctx: &ToolContext) {
        let Some(axis) = self.lock_axis.or(self.hover_axis) else {
            return;
        };
        Self::flip(vp, axis, self.copy);
        self.copy = false;
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.lock_axis = None;
        vp.update();
    }

    fn busy(&self) -> bool {
        false
    }
}
