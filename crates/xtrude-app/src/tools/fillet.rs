// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Fillet 3D tool: round the edges of a solid. Click an edge (or use the
//! selected ones), move away to set the radius, click again — or type the
//! radius; "Ns" sets the segments.

use std::cell::Cell;

use glam::DVec3;
use xtrude_core::fillet::{apply_fillet, plan_fillet, FilletPlan};
use xtrude_core::EdgeId;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_len_fine;

use super::{PreviewFace, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

thread_local! {
    /// The last radius used, shared across activations.
    static RADIUS: Cell<f64> = const { Cell::new(0.10) };
    static SEGMENTS: Cell<usize> = const { Cell::new(8) };
}

pub fn fillet_radius() -> f64 {
    RADIUS.with(|c| c.get())
}

pub fn set_fillet_radius(r: f64) {
    RADIUS.with(|c| c.set(r));
}

pub fn fillet_segments() -> usize {
    SEGMENTS.with(|c| c.get())
}

pub fn set_fillet_segments(n: usize) {
    SEGMENTS.with(|c| c.set(n));
}

type PlanKey = (Vec<EdgeId>, i64, usize, u64);

#[derive(Default)]
pub struct FilletTool {
    pub edges: Vec<EdgeId>,
    /// The cursor sets the radius.
    pub sizing: bool,
    pub hover_edge: Option<EdgeId>,
    pub hover_point: Option<DVec3>,
    plan: Option<FilletPlan>,
    plan_key: Option<PlanKey>,
    message: Option<String>,
}

/// The distance from `p` to the nearest of `edges`' lines; `None` when
/// none is left or `p` is on one.
pub(super) fn distance_to_edges(vp: &Viewport, edges: &[EdgeId], p: DVec3) -> Option<f64> {
    let mesh = vp.scene.mesh();
    let mut best: Option<f64> = None;
    for &e in edges {
        if !mesh.has_edge(e) {
            continue;
        }
        let (a, b) = mesh.edge_ab(e);
        let d = b - a;
        let ln = d.length();
        if ln < 1e-9 {
            continue;
        }
        let d = d / ln;
        let off = (p - a) - d * (p - a).dot(d);
        let r = off.length();
        if best.is_none_or(|b| r < b) {
            best = Some(r);
        }
    }
    best.filter(|b| *b >= 1e-4)
}

impl FilletTool {
    pub fn new() -> FilletTool {
        FilletTool::default()
    }

    fn reset(&mut self) {
        *self = FilletTool::new();
    }

    /// The loose edge under the cursor, if the hovered edge is one.
    pub(super) fn hovered_loose_edge(vp: &Viewport) -> Option<EdgeId> {
        let (m, e) = vp.inference.hover_edge.as_ref()?.edge?;
        (m == vp.scene.active_mesh_id() && vp.scene.mesh().has_edge(e)).then_some(e)
    }

    /// The hovered edge, joined by the selection when it is selected.
    pub(super) fn target_edges(vp: &Viewport) -> Vec<EdgeId> {
        let Some(e) = Self::hovered_loose_edge(vp) else {
            return Vec::new();
        };
        let mid = vp.scene.active_mesh_id();
        if vp.scene.selection.contains(&EntityRef::Edge(mid, e)) {
            let picked: Vec<EdgeId> = vp
                .scene
                .selection
                .iter()
                .filter_map(|x| match *x {
                    EntityRef::Edge(m, id) if m == mid => Some(id),
                    _ => None,
                })
                .collect();
            if !picked.is_empty() {
                return picked;
            }
        }
        vec![e]
    }

    fn current_edges(&self) -> Vec<EdgeId> {
        if !self.edges.is_empty() {
            return self.edges.clone();
        }
        self.hover_edge.into_iter().collect()
    }

    fn replan(&mut self, vp: &Viewport) {
        let edges = self.current_edges();
        if edges.is_empty() {
            self.plan = None;
            self.plan_key = None;
            self.message = None;
            return;
        }
        let mesh = vp.scene.mesh();
        let key = (
            edges.clone(),
            (fillet_radius() * 1e6).round() as i64,
            fillet_segments(),
            mesh.mut_serial,
        );
        if Some(&key) == self.plan_key.as_ref() {
            return;
        }
        self.plan_key = Some(key);
        match plan_fillet(mesh, &edges, fillet_radius(), fillet_segments()) {
            Ok(p) => {
                self.plan = Some(p);
                self.message = None;
            }
            Err(m) => {
                self.plan = None;
                self.message = Some(m);
            }
        }
    }

    fn radius_from(&self, vp: &Viewport, p: DVec3) -> Option<f64> {
        distance_to_edges(vp, &self.edges, p)
    }

    fn commit(&mut self, vp: &mut Viewport) {
        self.replan(vp);
        let Some(plan) = self.plan.clone() else {
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| tr("Nothing to round.", &[]));
            vp.flash_status(&msg, 4000);
            return;
        };
        let n = self.edges.len();
        let r = fillet_radius();
        vp.run(FnCommand::new(
            "SnapshotMutation",
            move |s: &mut xtrude_model::scene::Scene| {
                apply_fillet(s.mesh_mut(), &plan);
                s.version += 1;
                Ok(())
            },
        ));
        vp.scene.clear_selection();
        self.reset();
        vp.set_hover(None);
        let msg = tr(
            "Rounded {n} edge(s), radius {r}.",
            &[("n", &n), ("r", &fmt_len_fine(r))],
        );
        vp.flash_status(&msg, 3000);
        vp.update();
    }

    pub fn on_segments_value(&mut self, vp: &mut Viewport, n: u32) -> bool {
        if n < 1 {
            return false;
        }
        set_fillet_segments(n as usize);
        vp.flash_status(&tr("{n} segments", &[("n", &n)]), 2500);
        self.replan(vp);
        vp.update();
        true
    }
}

impl Tool for FilletTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("fillet"),
            description: Some("Round the edges of a solid to the radius you set."),
            wireframe_depth_tested: true,
            ..ToolInfo::new("fillet", "Fillet 3D")
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
        Some("Radius".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        let mid = vp.scene.active_mesh_id();
        let mesh = vp.scene.mesh();
        let picked: Vec<EdgeId> = vp
            .scene
            .selection
            .iter()
            .filter_map(|x| match *x {
                EntityRef::Edge(m, id) if m == mid && mesh.has_edge(id) => Some(id),
                _ => None,
            })
            .collect();
        if !picked.is_empty() {
            let msg = tr(
                "{n} edge(s) selected — type the radius, or click and move the cursor to set it.",
                &[("n", &picked.len())],
            );
            self.edges = picked;
            vp.flash_status(&msg, 5000);
        } else {
            vp.flash_status(
                &tr("Click an edge to round it; type the radius and Enter.", &[]),
                4000,
            );
        }
        self.replan(vp);
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.sizing {
            self.commit(vp);
            return;
        }
        if self.edges.is_empty() {
            self.edges = Self::target_edges(vp);
            if self.edges.is_empty() {
                return;
            }
        }
        self.sizing = true;
        self.hover_point = Some(ctx.world);
        self.replan(vp);
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
        if self.sizing {
            if let Some(r) = self.radius_from(vp, ctx.world) {
                set_fillet_radius(r);
            }
            self.replan(vp);
            vp.update();
            return;
        }
        if self.edges.is_empty() {
            self.hover_edge = Self::hovered_loose_edge(vp);
            let mid = vp.scene.active_mesh_id();
            vp.set_hover(self.hover_edge.map(|e| EntityRef::Edge(mid, e)));
            self.replan(vp);
        }
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            segments: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        if let ToolValue::Segments(n) = *value {
            return self.on_segments_value(vp, n);
        }
        let Some(v) = value.as_num() else {
            return false;
        };
        if v <= 0.0 {
            return false;
        }
        set_fillet_radius(v);
        if !self.edges.is_empty() {
            self.commit(vp);
        } else {
            vp.flash_status(
                &tr("Radius {r} — click an edge.", &[("r", &fmt_len_fine(v))]),
                3000,
            );
            self.replan(vp);
            vp.update();
        }
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
        vp.update();
    }

    fn preview_faces(&self) -> Vec<PreviewFace> {
        let Some(p) = &self.plan else {
            return Vec::new();
        };
        p.new_faces()
            .into_iter()
            .map(|lp| PreviewFace {
                vertices: lp,
                holes: Vec::new(),
                attrs: None,
            })
            .collect()
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(p) = &self.plan else {
            return Vec::new();
        };
        p.curves
            .iter()
            .flat_map(|pts| pts.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())
            .collect()
    }

    fn value_label(&self, vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let e = *self.current_edges().first()?;
        let mesh = vp.scene.mesh();
        if !mesh.has_edge(e) {
            return None;
        }
        let (a, b) = mesh.edge_ab(e);
        let mid = (a + b) * 0.5;
        if let Some(m) = &self.message {
            return Some((m.clone(), Some(mid)));
        }
        let segs = tr("{n} segments", &[("n", &fillet_segments())]);
        Some((
            format!("R {}  ({segs})", fmt_len_fine(fillet_radius())),
            Some(mid),
        ))
    }

    /// Esc cascade: a rounding in progress.
    fn busy(&self) -> bool {
        !self.edges.is_empty() || self.sizing
    }

    fn busy_for_delete(&self) -> bool {
        self.busy()
    }
    fn phase(&self) -> &'static str {
        if self.sizing {
            "sizing"
        } else {
            "idle"
        }
    }
}
