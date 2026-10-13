// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Chamfer tool: bevel the edges of a solid. Click an edge (or use the
//! selected ones), move away to set the distance, click again — or type
//! the distance. The distance is measured along each face from the edge.

use std::cell::Cell;

use glam::DVec3;
use xtrude_core::fillet::{apply_fillet, plan_chamfer, FilletPlan};
use xtrude_core::EdgeId;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_len_fine;

use super::fillet::{distance_to_edges, FilletTool};
use super::{PreviewFace, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::tool_any;
use crate::viewport::Viewport;

thread_local! {
    /// The last distance used, shared across activations.
    static DISTANCE: Cell<f64> = const { Cell::new(0.001) };
}

pub fn chamfer_distance() -> f64 {
    DISTANCE.with(|c| c.get())
}

pub fn set_chamfer_distance(d: f64) {
    DISTANCE.with(|c| c.set(d));
}

type PlanKey = (Vec<EdgeId>, i64, u64);

#[derive(Default)]
pub struct ChamferTool {
    pub edges: Vec<EdgeId>,
    /// The cursor sets the distance.
    pub sizing: bool,
    pub hover_edge: Option<EdgeId>,
    plan: Option<FilletPlan>,
    plan_key: Option<PlanKey>,
    message: Option<String>,
}

impl ChamferTool {
    pub fn new() -> ChamferTool {
        ChamferTool::default()
    }

    fn reset(&mut self) {
        *self = ChamferTool::new();
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
            (chamfer_distance() * 1e6).round() as i64,
            mesh.mut_serial,
        );
        if Some(&key) == self.plan_key.as_ref() {
            return;
        }
        self.plan_key = Some(key);
        match plan_chamfer(mesh, &edges, chamfer_distance()) {
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

    fn commit(&mut self, vp: &mut Viewport) {
        self.replan(vp);
        let Some(plan) = self.plan.clone() else {
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| tr("Nothing to chamfer.", &[]));
            vp.flash_status(&msg, 4000);
            return;
        };
        let n = self.edges.len();
        let d = chamfer_distance();
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
            "Chamfered {n} edge(s), distance {d}.",
            &[("n", &n), ("d", &fmt_len_fine(d))],
        );
        vp.flash_status(&msg, 3000);
        vp.update();
    }
}

impl Tool for ChamferTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("chamfer"),
            description: Some("Bevel the edges of a solid by the distance you set."),
            wireframe_depth_tested: true,
            ..ToolInfo::new("chamfer", "Chamfer")
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
        Some("Distance".into())
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
                "{n} edge(s) selected — type the distance, or click and move the cursor to set it.",
                &[("n", &picked.len())],
            );
            self.edges = picked;
            vp.flash_status(&msg, 5000);
        } else {
            vp.flash_status(
                &tr(
                    "Click an edge to chamfer it; type the distance and Enter.",
                    &[],
                ),
                4000,
            );
        }
        self.replan(vp);
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
    }

    fn on_click(&mut self, vp: &mut Viewport, _ctx: &ToolContext) {
        if self.sizing {
            self.commit(vp);
            return;
        }
        if self.edges.is_empty() {
            self.edges = FilletTool::target_edges(vp);
            if self.edges.is_empty() {
                return;
            }
        }
        self.sizing = true;
        self.replan(vp);
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.sizing {
            if let Some(d) = distance_to_edges(vp, &self.edges, ctx.world) {
                set_chamfer_distance(d);
            }
            self.replan(vp);
            vp.update();
            return;
        }
        if self.edges.is_empty() {
            self.hover_edge = FilletTool::hovered_loose_edge(vp);
            let mid = vp.scene.active_mesh_id();
            vp.set_hover(self.hover_edge.map(|e| EntityRef::Edge(mid, e)));
            self.replan(vp);
        }
        vp.update();
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(v) = value.as_num() else {
            return false;
        };
        if v <= 0.0 {
            return false;
        }
        set_chamfer_distance(v);
        if !self.edges.is_empty() {
            self.commit(vp);
        } else {
            vp.flash_status(
                &tr("Distance {d} — click an edge.", &[("d", &fmt_len_fine(v))]),
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
        Some((fmt_len_fine(chamfer_distance()), Some(mid)))
    }

    /// Esc cascade: a chamfer in progress.
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
