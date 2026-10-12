// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Offset tool: offset a face's boundary in its plane — the face splits into a
//! ring and an inner face (walls with thickness) — or a run of connected
//! coplanar edges.

use crate::input::ScreenPt;
use glam::DVec3;
use xtrude_core::math::VecExt;
use xtrude_core::offset::{offset_chain, offset_regions};
use xtrude_core::topology::{max_offset_distance, offset_loop};
use xtrude_core::{EdgeId, FaceId, Mesh, VertexId};
use xtrude_model::commands::{AddFaceCommand, DeleteFaceCommand, TagCurveCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;
use xtrude_model::units::fmt_len;

use super::{Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::tool_any;
use crate::viewport::Viewport;

fn point_segment_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-12 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Consecutive pairs of `points`, wrapping when `closed`.
pub fn segments(points: &[DVec3], closed: bool) -> Vec<(DVec3, DVec3)> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let last = if closed { n } else { n - 1 };
    (0..last)
        .map(|i| (points[i], points[(i + 1) % n]))
        .collect()
}

/// The plane normal of a polyline (Newell), or `None` when in line.
pub fn newell(points: &[DVec3]) -> Option<DVec3> {
    let n = points.len();
    if n < 3 {
        return None;
    }
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (points[i], points[(i + 1) % n]);
        nx += (p.y - q.y) * (p.z + q.z);
        ny += (p.z - q.z) * (p.x + q.x);
        nz += (p.x - q.x) * (p.y + q.y);
    }
    let normal = DVec3::new(nx, ny, nz);
    let span = points
        .iter()
        .map(|p| (*p - points[0]).length())
        .fold(0.0, f64::max);
    let span = if span == 0.0 { 1.0 } else { span };
    if normal.length() < 1e-6 * span * span {
        return None;
    }
    Some(normal.unit_or_zero())
}

/// Order a set of edges into one run `(points, closed)`, or `None`.
pub fn run_from_edges(mesh: &Mesh, edges: &[EdgeId]) -> Option<(Vec<DVec3>, bool)> {
    let mut nbrs: Vec<(VertexId, Vec<EdgeId>)> = Vec::new();
    for &e in edges {
        let ed = mesh.edge(e);
        for v in [ed.v0, ed.v1] {
            match nbrs.iter_mut().find(|x| x.0 == v) {
                Some(x) => x.1.push(e),
                None => nbrs.push((v, vec![e])),
            }
        }
    }
    if nbrs.iter().any(|x| x.1.len() > 2) {
        return None;
    }
    let ends: Vec<VertexId> = nbrs
        .iter()
        .filter(|x| x.1.len() == 1)
        .map(|x| x.0)
        .collect();
    if !ends.is_empty() && ends.len() != 2 {
        return None;
    }
    let closed = ends.is_empty();
    let start = if closed {
        mesh.edge(edges[0]).v0
    } else {
        ends[0]
    };
    let mut points = vec![mesh.pos(start)];
    let mut seen: Vec<EdgeId> = Vec::new();
    let mut v = start;
    loop {
        let list = &nbrs.iter().find(|x| x.0 == v)?.1;
        let Some(&nxt) = list.iter().find(|e| !seen.contains(e)) else {
            break;
        };
        seen.push(nxt);
        v = mesh.edge(nxt).other(v);
        if closed && v == start {
            break;
        }
        points.push(mesh.pos(v));
    }
    if seen.len() != edges.len() {
        return None;
    }
    Some((points, closed))
}

/// The run `edge` belongs to, walked both ways through two-edge vertices.
pub fn run_from_edge(mesh: &Mesh, edge: EdgeId) -> (Vec<DVec3>, bool) {
    let walk = |start_v: VertexId| -> (Vec<DVec3>, bool) {
        let mut out = Vec::new();
        let (mut v, mut e) = (start_v, edge);
        while mesh.verts[v].edges.len() == 2 {
            let next = mesh.verts[v].edges.iter().copied().find(|&x| x != e);
            let Some(next) = next else { break };
            e = next;
            if e == edge {
                return (out, true);
            }
            v = mesh.edge(e).other(v);
            out.push(mesh.pos(v));
        }
        (out, false)
    };
    let ed = mesh.edge(edge);
    let (p0, p1) = (mesh.pos(ed.v0), mesh.pos(ed.v1));
    let (fwd, closed) = walk(ed.v1);
    if closed {
        let mut pts = vec![p0, p1];
        pts.extend(fwd[..fwd.len().saturating_sub(1)].iter().copied());
        return (pts, true);
    }
    let (back, _) = walk(ed.v0);
    let mut pts: Vec<DVec3> = back.into_iter().rev().collect();
    pts.push(p0);
    pts.push(p1);
    pts.extend(fwd);
    (pts, false)
}

/// The bounding box's squared diagonal (ranks regions).
fn loop_extent(lp: &[DVec3]) -> f64 {
    let (lo, hi) = lp.iter().fold(
        (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    (hi - lo).length_squared()
}

#[derive(Default)]
pub struct OffsetTool {
    pub hovered_face: Option<(MeshId, FaceId)>,
    pub hovered_edge: Option<(MeshId, EdgeId)>,
    pub base_face: Option<(MeshId, FaceId)>,
    /// Signed: > 0 inward, < 0 outward.
    pub distance: f64,
    pub dragging: bool,
    pub lp: Vec<DVec3>,
    pub normal: Option<DVec3>,
    ref_point: Option<DVec3>,
    ref_inward: Option<DVec3>,
    /// Edges mode: `lp` is a run of edges.
    pub chain: bool,
    pub closed: bool,
    /// A run taken from the selection: the first click starts the offset.
    pub armed: bool,
}

impl OffsetTool {
    pub fn new() -> OffsetTool {
        OffsetTool {
            closed: true,
            ..Default::default()
        }
    }

    fn reset(&mut self) {
        *self = OffsetTool::new();
    }

    fn offset_points(&self) -> Option<Vec<DVec3>> {
        let n = self.normal?;
        if self.closed {
            offset_loop(&self.lp, n, self.distance)
        } else {
            offset_chain(&self.lp, n, self.distance)
        }
    }

    fn cursor_on_plane(&self, vp: &Viewport, sx: f64, sy: f64) -> Option<DVec3> {
        let (o, d) = vp.pixel_to_ray(ScreenPt::new(sx, sy))?;
        let n = self.normal?;
        let denom = d.dot(n);
        if denom.abs() < 1e-9 {
            return None;
        }
        let anchor = match self.base_face {
            Some((m, f)) if vp.scene.doc.mesh(m).has_face(f) => {
                vp.scene.doc.mesh(m).face_centroid(f)
            }
            _ => *self.lp.first()?,
        };
        let t = (anchor - o).dot(n) / denom;
        Some(o + d * t)
    }

    /// Lock onto the boundary edge nearest the click.
    fn pick_reference(&mut self, vp: &Viewport, sx: f64, sy: f64) {
        let cursor = self.cursor_on_plane(vp, sx, sy);
        let segs = segments(&self.lp, self.closed);
        if segs.is_empty() {
            return;
        }
        let mut best = (0usize, f64::INFINITY);
        for (i, (a, b)) in segs.iter().enumerate() {
            let d = cursor.map_or(0.0, |c| point_segment_distance(c, *a, *b));
            if d < best.1 {
                best = (i, d);
            }
        }
        let (a, b) = segs[best.0];
        let e = (b - a).unit_or_zero();
        self.ref_point = Some(a);
        self.ref_inward = self
            .normal
            .map(|n| n.unit_or_zero().cross(e).unit_or_zero());
    }

    /// Selected edges (and nothing else) arm the tool.
    fn arm_from_selection(&mut self, vp: &mut Viewport) {
        let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
        let mid = vp.scene.active_mesh_id();
        let edges: Vec<EdgeId> = sel
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Edge(m, id) if m == mid => Some(id),
                _ => None,
            })
            .collect();
        if edges.len() < 2 || edges.len() != sel.len() {
            return;
        }
        let Some(run) = run_from_edges(vp.scene.mesh(), &edges) else {
            return;
        };
        if self.take_run(vp, Some(run)) {
            self.armed = true;
            let msg = tr(
                "{n} edges selected — click to start the offset",
                &[("n", &edges.len())],
            );
            vp.flash_status(&msg, 4000);
        }
    }

    fn take_run(&mut self, vp: &mut Viewport, run: Option<(Vec<DVec3>, bool)>) -> bool {
        let Some((points, closed)) = run else {
            vp.flash_status(
                &tr(
                    "Offset needs one connected run of edges — a polyline, or a closed loop.",
                    &[],
                ),
                5000,
            );
            return false;
        };
        let Some(normal) = newell(&points) else {
            if points.len() == 2 && !closed {
                vp.flash_status(
                    &tr(
                        "This line is on its own — nothing is joined to its ends, so there is no chain to offset. Draw the missing line, or select the edges to offset.",
                        &[],
                    ),
                    6000,
                );
                if let Some((m, e)) = self.hovered_edge {
                    vp.scene
                        .select([EntityRef::Edge(m, e)], SelectMode::Replace);
                }
                return false;
            }
            vp.flash_status(
                &tr("Offset needs at least two edges that are not in line — a straight run has no plane to offset in.", &[]),
                5000,
            );
            return false;
        };
        self.base_face = None;
        self.lp = points;
        self.normal = Some(normal);
        self.chain = true;
        self.closed = closed;
        true
    }

    fn curve_tags(&self, mesh: &Mesh, off: &[DVec3]) -> Vec<Box<dyn Command>> {
        let n = self.lp.len();
        let ids: Vec<Option<i64>> = (0..n)
            .map(|i| {
                let va = mesh.vertex_at(self.lp[i]);
                let vb = mesh.vertex_at(self.lp[(i + 1) % n]);
                match (va, vb) {
                    (Some(a), Some(b)) => mesh.find_edge(a, b).and_then(|e| mesh.edge(e).curve),
                    _ => None,
                }
            })
            .collect();
        if ids[0].is_some() && ids.iter().all(|c| *c == ids[0]) {
            return vec![Box::new(TagCurveCommand {
                pts: off.to_vec(),
                closed: true,
            })];
        }
        let mut tags: Vec<Box<dyn Command>> = Vec::new();
        for k in 0..n {
            if ids[k].is_none() || ids[(k + n - 1) % n] == ids[k] {
                continue;
            }
            let mut pts = vec![off[k]];
            let mut j = k;
            while ids[j] == ids[k] {
                pts.push(off[(j + 1) % n]);
                j = (j + 1) % n;
                if j == k {
                    break;
                }
            }
            tags.push(Box::new(TagCurveCommand { pts, closed: false }));
        }
        tags
    }

    /// Commit the offset as set up.
    pub fn commit_now(&mut self, vp: &mut Viewport) {
        self.commit(vp);
    }

    fn commit(&mut self, vp: &mut Viewport) {
        if self.chain {
            self.commit_chain(vp);
            return;
        }
        let Some(n) = self.normal else { return };
        let mut regions = offset_regions(&self.lp, n, self.distance);
        if regions.is_empty() {
            if let Some(fb) = offset_loop(&self.lp, n, self.distance) {
                regions = vec![(fb, Vec::new())];
            }
        }
        if regions.is_empty() {
            self.refuse(vp);
            return;
        }
        let attrs = self
            .base_face
            .filter(|(m, f)| vp.scene.doc.mesh(*m).has_face(*f))
            .map(|(m, f)| vp.scene.doc.mesh(m).face(f).attrs.clone())
            .filter(|a| !a.is_empty());
        let (outer, inners): (Vec<DVec3>, Vec<Vec<DVec3>>) = if self.distance > 0.0 {
            (
                self.lp.clone(),
                regions.iter().map(|r| r.0.clone()).collect(),
            )
        } else {
            let o = regions
                .iter()
                .map(|r| &r.0)
                .max_by(|a, b| loop_extent(a).total_cmp(&loop_extent(b)))
                .cloned()
                .unwrap_or_default();
            (o, vec![self.lp.clone()])
        };
        let mut cmds: Vec<Box<dyn Command>> = vec![
            Box::new(DeleteFaceCommand {
                lp: self.lp.clone(),
            }),
            Box::new(AddFaceCommand::plain(outer, inners.clone(), attrs.clone())),
        ];
        for lp in &inners {
            cmds.push(Box::new(AddFaceCommand::plain(
                lp.clone(),
                Vec::new(),
                attrs.clone(),
            )));
        }
        if regions.len() == 1 && regions[0].0.len() == self.lp.len() {
            let off = regions[0].0.clone();
            cmds.extend(self.curve_tags(vp.scene.mesh(), &off));
        }
        vp.execute(&mut CompoundCommand::new(cmds));
        self.reset();
        vp.update();
    }

    fn commit_chain(&mut self, vp: &mut Viewport) {
        let Some(off) = self.offset_points() else {
            let msg = tr(
                "{d} collapses this run — try a smaller offset",
                &[("d", &fmt_len(self.distance.abs()))],
            );
            vp.flash_status(&msg, 5000);
            self.reset();
            vp.update();
            return;
        };
        let eye = Some(vp.camera.eye());
        let cmd = build_add_edges(
            &vp.scene,
            &segments(&off, self.closed),
            true,
            Vec::new(),
            eye,
        );
        vp.execute_boxed(cmd);
        self.reset();
        vp.update();
    }

    fn refuse(&mut self, vp: &mut Viewport) {
        let sign = if self.distance < 0.0 { -1.0 } else { 1.0 };
        let room = max_offset_distance(&self.lp, self.normal.unwrap_or(DVec3::Z), sign, 10.0);
        let side = tr(if sign > 0.0 { "inward" } else { "outward" }, &[]);
        let d = fmt_len(self.distance.abs());
        let msg = if room <= 1e-4 {
            tr(
                "{d} closes this face — it takes no offset {side}",
                &[("d", &d), ("side", &side)],
            )
        } else {
            tr(
                "{d} closes this face — {side} it takes at most {max}",
                &[("d", &d), ("side", &side), ("max", &fmt_len(room))],
            )
        };
        vp.flash_status(&msg, 5000);
        self.reset();
        vp.update();
    }
}

impl Tool for OffsetTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("offset"),
            shortcut: Some("F"),
            description: Some("Draw a copy of a face's outline at an even distance inside or outside it — walls with thickness."),
            wireframe_depth_tested: true,
            ..ToolInfo::new("offset", "Offset")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Offset".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        self.arm_from_selection(vp);
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
        self.reset();
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if !self.dragging {
            if self.armed {
                return;
            }
            self.hovered_face = vp.pick_face(ctx.screen.0, ctx.screen.1);
            self.hovered_edge = None;
            if self.hovered_face.is_none() {
                self.hovered_edge = vp
                    .pick_edge(ctx.screen.0, ctx.screen.1)
                    .and_then(|e| e.edge);
            }
            let hover = self
                .hovered_face
                .map(|(m, f)| EntityRef::Face(m, f))
                .or(self.hovered_edge.map(|(m, e)| EntityRef::Edge(m, e)));
            vp.set_hover(hover);
            return;
        }
        if let (Some(c), Some(rp), Some(ri)) = (
            self.cursor_on_plane(vp, ctx.screen.0, ctx.screen.1),
            self.ref_point,
            self.ref_inward,
        ) {
            self.distance = (c - rp).dot(ri);
        }
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if !self.dragging {
            if self.armed {
                self.armed = false;
            } else if let Some((m, f)) = self.hovered_face {
                let mesh = vp.scene.doc.mesh(m);
                if !mesh.has_face(f) || mesh.face(f).outer.len() < 3 {
                    return;
                }
                self.base_face = Some((m, f));
                self.lp = mesh.face_positions(f);
                self.normal = Some(mesh.face_normal(f));
                self.chain = false;
                self.closed = true;
            } else if let Some((m, e)) = self.hovered_edge {
                let run = run_from_edge(vp.scene.doc.mesh(m), e);
                if !self.take_run(vp, Some(run)) {
                    return;
                }
            } else {
                return;
            }
            self.distance = 0.0;
            self.dragging = true;
            self.pick_reference(vp, ctx.screen.0, ctx.screen.1);
            vp.set_hover(None);
            vp.update();
            return;
        }
        if self.distance.abs() < 1e-6 {
            return;
        }
        self.commit(vp);
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(v) = value.as_num() else {
            return false;
        };
        if !self.dragging || self.lp.is_empty() || v <= 0.0 {
            return false;
        }
        let sign = if self.distance < 0.0 { -1.0 } else { 1.0 };
        self.distance = sign * v;
        self.commit(vp);
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        if !self.dragging || self.lp.is_empty() || self.distance.abs() < 1e-6 {
            return Vec::new();
        }
        self.offset_points()
            .map_or(Vec::new(), |off| segments(&off, self.closed))
    }

    fn busy(&self) -> bool {
        self.dragging
    }

    fn busy_for_delete(&self) -> bool {
        self.dragging || self.base_face.is_some()
    }
}
