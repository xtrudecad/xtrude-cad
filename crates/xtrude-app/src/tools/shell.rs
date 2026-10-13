// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shell tool: hollow a solid to a wall thickness. Click a solid, then
//! click the faces to leave open (a second click closes one again); Enter
//! or a typed thickness applies. With no open face the solid gets a closed
//! cavity. Faces selected before the tool starts are its open faces.

use std::cell::Cell;

use glam::DVec3;
use xtrude_core::shell::{apply_shell, plan_shell, ShellPlan};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::i18n::tr;
use xtrude_model::pieces::connected_parts;
use xtrude_model::units::fmt_len_fine;

use super::{PreviewFace, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::Viewport;

thread_local! {
    /// The last wall thickness used, shared across activations.
    static THICKNESS: Cell<f64> = const { Cell::new(0.002) };
}

pub fn shell_thickness() -> f64 {
    THICKNESS.with(|c| c.get())
}

pub fn set_shell_thickness(t: f64) {
    THICKNESS.with(|c| c.set(t));
}

type PlanKey = (Vec<FaceId>, i64, u64);

#[derive(Default)]
pub struct ShellTool {
    /// The faces of the solid picked; empty until the first click.
    pub solid: Vec<FaceId>,
    /// The faces left open, in the order clicked.
    pub open: Vec<FaceId>,
    pub hover_face: Option<FaceId>,
    plan: Option<ShellPlan>,
    plan_key: Option<PlanKey>,
    message: Option<String>,
}

/// The boundary faces of the piece of `mesh` that holds `face`.
fn solid_of(mesh: &Mesh, face: FaceId) -> Vec<FaceId> {
    connected_parts(mesh)
        .into_iter()
        .find(|(faces, _)| faces.contains(&face))
        .map(|(faces, _)| {
            faces
                .into_iter()
                .filter(|&f| !mesh.face(f).interior)
                .collect()
        })
        .unwrap_or_default()
}

impl ShellTool {
    pub fn new() -> ShellTool {
        ShellTool::default()
    }

    fn reset(&mut self) {
        *self = ShellTool::new();
    }

    /// The active mesh's face under the cursor.
    fn hovered_face(vp: &mut Viewport, ctx: &ToolContext) -> Option<FaceId> {
        let (m, f) = vp.pick_face(ctx.screen.0, ctx.screen.1)?;
        (m == vp.scene.active_mesh_id() && vp.scene.mesh().has_face(f)).then_some(f)
    }

    fn replan(&mut self, vp: &Viewport) {
        if self.solid.is_empty() {
            self.plan = None;
            self.plan_key = None;
            self.message = None;
            return;
        }
        let mesh = vp.scene.mesh();
        let key = (
            self.open.clone(),
            (shell_thickness() * 1e6).round() as i64,
            mesh.mut_serial,
        );
        if Some(&key) == self.plan_key.as_ref() {
            return;
        }
        self.plan_key = Some(key);
        match plan_shell(mesh, &self.solid, &self.open, shell_thickness()) {
            Ok(p) => {
                self.plan = Some(p);
                self.message = None;
            }
            Err(e) => {
                self.plan = None;
                self.message = Some(e.to_string());
            }
        }
    }

    fn commit(&mut self, vp: &mut Viewport) {
        self.replan(vp);
        let Some(plan) = self.plan.clone() else {
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| tr("Click a solid to hollow.", &[]));
            vp.flash_status(&msg, 4000);
            return;
        };
        let (n, t) = (self.open.len(), shell_thickness());
        vp.run(FnCommand::new(
            "SnapshotMutation",
            move |s: &mut xtrude_model::scene::Scene| {
                apply_shell(s.mesh_mut(), &plan);
                s.version += 1;
                Ok(())
            },
        ));
        vp.scene.clear_selection();
        self.reset();
        vp.set_hover(None);
        let msg = tr(
            "Hollowed the solid, wall {t}, {n} face(s) open.",
            &[("t", &fmt_len_fine(t)), ("n", &n)],
        );
        vp.flash_status(&msg, 3000);
        vp.update();
    }

    /// Pick the solid of `face`, or open (or close again) one of its faces.
    fn pick(&mut self, vp: &mut Viewport, face: FaceId) {
        if self.solid.is_empty() {
            self.solid = solid_of(vp.scene.mesh(), face);
            vp.flash_status(
                &tr(
                    "Click the faces to leave open, then Enter, or type the wall thickness.",
                    &[],
                ),
                5000,
            );
        } else if !self.solid.contains(&face) {
            return;
        } else if let Some(i) = self.open.iter().position(|&f| f == face) {
            self.open.remove(i);
        } else {
            self.open.push(face);
        }
        self.replan(vp);
        vp.update();
    }
}

impl Tool for ShellTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("shell"),
            description: Some(
                "Hollow a solid to the wall thickness you set, leaving the faces you click open.",
            ),
            wireframe_depth_tested: false,
            ..ToolInfo::new("shell", "Shell")
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
        Some("Thickness".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        let mid = vp.scene.active_mesh_id();
        let mesh = vp.scene.mesh();
        let picked: Vec<FaceId> = vp
            .scene
            .selection
            .iter()
            .filter_map(|x| match *x {
                EntityRef::Face(m, id) if m == mid && mesh.has_face(id) => Some(id),
                _ => None,
            })
            .collect();
        if let Some(&first) = picked.first() {
            self.solid = solid_of(mesh, first);
            self.open = picked
                .into_iter()
                .filter(|f| self.solid.contains(f))
                .collect();
            vp.flash_status(
                &tr(
                    "{n} face(s) open — type the wall thickness, or click more faces.",
                    &[("n", &self.open.len())],
                ),
                5000,
            );
        } else {
            vp.flash_status(&tr("Click a solid to hollow it.", &[]), 4000);
        }
        self.replan(vp);
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if let Some(f) = Self::hovered_face(vp, ctx) {
            self.pick(vp, f);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_face = Self::hovered_face(vp, ctx);
        let mid = vp.scene.active_mesh_id();
        vp.set_hover(self.hover_face.map(|f| EntityRef::Face(mid, f)));
        vp.update();
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if matches!(key, Key::Return | Key::Enter) && !self.solid.is_empty() {
            self.commit(vp);
            return true;
        }
        false
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let Some(v) = value.as_num() else {
            return false;
        };
        if v <= 0.0 {
            return false;
        }
        set_shell_thickness(v);
        if !self.solid.is_empty() {
            self.commit(vp);
        } else {
            vp.flash_status(
                &tr("Wall {t} — click a solid.", &[("t", &fmt_len_fine(v))]),
                3000,
            );
        }
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
        vp.update();
    }

    /// The rims of the open faces; the cavity shows as lines.
    fn preview_faces(&self) -> Vec<PreviewFace> {
        let Some(p) = &self.plan else {
            return Vec::new();
        };
        p.rims
            .iter()
            .map(|f| PreviewFace {
                vertices: f.outer.clone(),
                holes: f.holes.clone(),
                attrs: None,
            })
            .collect()
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(p) = &self.plan else {
            return Vec::new();
        };
        p.cavity
            .iter()
            .flat_map(|f| std::iter::once(&f.outer).chain(f.holes.iter()))
            .flat_map(|lp| (0..lp.len()).map(|i| (lp[i], lp[(i + 1) % lp.len()])))
            .collect()
    }

    fn value_label(&self, vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let &f = self.solid.first()?;
        let mesh = vp.scene.mesh();
        if !mesh.has_face(f) {
            return None;
        }
        let at = Some(mesh.face_centroid(self.open.first().copied().unwrap_or(f)));
        if let Some(m) = &self.message {
            return Some((m.clone(), at));
        }
        Some((fmt_len_fine(shell_thickness()), at))
    }

    /// Esc cascade: a solid picked.
    fn busy(&self) -> bool {
        !self.solid.is_empty()
    }

    fn busy_for_delete(&self) -> bool {
        self.busy()
    }
    fn phase(&self) -> &'static str {
        if self.solid.is_empty() {
            "idle"
        } else {
            "faces"
        }
    }
}
