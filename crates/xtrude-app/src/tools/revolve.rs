// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Revolve tool: turn a face about an axis in its plane.
//!
//! Click the face (or select it first), then the axis: an edge in the
//! face's plane, or two points (read on the face's plane, a snapped point
//! off it brought onto it). The revolve shows live; a click turns it by
//! the angle (360° until another is typed), a typed angle turns it by
//! that. `Ns` sets the sides of a full turn (24). The angle and the sides
//! are kept between uses. An axis that runs through the face, or one off
//! its plane, is refused with the reason. The revolve itself is
//! `xtrude_core::sweep::revolve_profile`: a full turn sweeps the lathe
//! path, a partial one an arc of it. One undo step.

use glam::DVec3;
use xtrude_core::geometry::PolyFace;
use xtrude_core::math::VecExt;
use xtrude_core::sweep::{
    revolve_angle_ok, revolve_check, revolve_preview_faces, revolve_profile, RevolveRefusal,
    REVOLVE_SIDES,
};
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::i18n::tr;
use xtrude_model::ids::MeshId;

use super::{Plane, PreviewFace, Rgba, Tool, ToolContext, ToolInfo, ToolValue};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::Viewport;

/// The angle a revolve turns by until another is typed.
pub const DEFAULT_ANGLE: f64 = 360.0;
/// How far (m) an edge's ends may stand off the face's plane and still
/// make the axis.
const ON_PLANE: f64 = 1e-6;

/// What the user is told when a revolve is refused.
pub fn refusal_text(why: RevolveRefusal) -> String {
    tr(
        match why {
            RevolveRefusal::OffPlane => "Revolve: the axis must lie in the face's plane",
            RevolveRefusal::CrossesFace => {
                "Revolve: the axis runs through the face; pick one beside it"
            }
            RevolveRefusal::Angle => "Revolve: the angle must be above 0° and at most 360°",
            RevolveRefusal::Degenerate => "Revolve: this face cannot be turned",
        },
        &[],
    )
}

pub struct RevolveTool {
    /// The face to turn (in the active mesh).
    pub face: Option<FaceId>,
    pub hovered_face: Option<FaceId>,
    /// The first point of a two-point axis.
    pub axis_start: Option<DVec3>,
    /// The axis: a point on it and its unit direction.
    pub axis: Option<(DVec3, DVec3)>,
    pub hover: Option<DVec3>,
    /// The angle the next revolve turns by, degrees (signed).
    pub angle: f64,
    /// The sides of a full turn.
    pub sides: u32,
    plane: Option<Plane>,
    preview: Vec<PreviewFace>,
}

impl Default for RevolveTool {
    fn default() -> RevolveTool {
        RevolveTool {
            face: None,
            hovered_face: None,
            axis_start: None,
            axis: None,
            hover: None,
            angle: DEFAULT_ANGLE,
            sides: REVOLVE_SIDES,
            plane: None,
            preview: Vec::new(),
        }
    }
}

impl RevolveTool {
    pub fn new() -> RevolveTool {
        RevolveTool::default()
    }

    fn reset(&mut self) {
        self.face = None;
        self.axis_start = None;
        self.axis = None;
        self.hover = None;
        self.plane = None;
        self.preview.clear();
    }

    fn profile(&self, vp: &Viewport) -> Option<PolyFace> {
        let f = self.face?;
        let m = vp.scene.mesh();
        m.has_face(f).then(|| m.poly_face(f))
    }

    /// `p` brought onto the face's plane.
    fn on_plane(&self, p: DVec3) -> DVec3 {
        match self.plane {
            Some((o, n)) => p - n * (p - o).dot(n),
            None => p,
        }
    }

    fn take_face(&mut self, vp: &mut Viewport, f: FaceId) {
        let m = vp.scene.mesh();
        self.face = Some(f);
        self.plane = Some((m.face_centroid(f), m.face_normal(f).unit_or_zero()));
        self.axis_start = None;
        self.axis = None;
        vp.set_hover(None);
        self.hovered_face = None;
    }

    /// The one loose face selected, if that is the selection.
    fn selected_face(vp: &Viewport) -> Option<FaceId> {
        let mid = vp.scene.active_mesh_id();
        let sel: Vec<&EntityRef> = vp.scene.selection.iter().collect();
        match sel.as_slice() {
            [EntityRef::Face(m, f)] if *m == mid => Some(*f),
            _ => None,
        }
    }

    /// The edge under the cursor as an axis, when it lies in the face's
    /// plane.
    fn edge_axis(&self, vp: &mut Viewport, ctx: &ToolContext) -> Option<(DVec3, DVec3)> {
        let (o, n) = self.plane?;
        let e = vp.pick_edge(ctx.screen.0, ctx.screen.1)?;
        let on = |p: DVec3| (p - o).dot(n).abs() < ON_PLANE;
        let d = e.b - e.a;
        (on(e.a) && on(e.b) && d.length() > 1e-9).then(|| (e.a, d.unit_or_zero()))
    }

    /// Take `axis`, or refuse it with the reason (back to picking one).
    fn set_axis(&mut self, vp: &mut Viewport, axis: (DVec3, DVec3)) {
        self.axis_start = None;
        let Some(pf) = self.profile(vp) else {
            self.reset();
            return;
        };
        if let Err(why) = revolve_check(&pf, axis.0, axis.1) {
            self.axis = None;
            vp.flash_status(&refusal_text(why), 4000);
            vp.update();
            return;
        }
        self.axis = Some(axis);
        self.rebuild_preview(vp);
    }

    fn rebuild_preview(&mut self, vp: &mut Viewport) {
        self.preview = match (self.profile(vp), self.axis) {
            (Some(pf), Some((o, d))) => revolve_preview_faces(&pf, o, d, self.angle, self.sides),
            _ => Vec::new(),
        };
        vp.update();
    }

    /// Turn the face by `deg` about the axis.
    fn commit(&mut self, vp: &mut Viewport, deg: f64) {
        let (Some(face), Some((o, d))) = (self.face, self.axis) else {
            return;
        };
        let sides = self.sides;
        let res = std::cell::Cell::new(Ok(()));
        vp.run(FnCommand::new(
            "SnapshotMutation",
            |s: &mut xtrude_model::scene::Scene| {
                res.set(revolve_profile(s.mesh_mut(), face, o, d, deg, sides));
                Ok(())
            },
        ));
        if let Err(why) = res.get() {
            vp.history.discard_last(&mut vp.scene);
            vp.flash_status(&refusal_text(why), 4000);
        } else {
            vp.scene.selection.clear();
        }
        self.reset();
        vp.update();
    }
}

impl Tool for RevolveTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("revolve"),
            description: Some(
                "Turn a face about an axis in its plane: a full or partial turned shape.",
            ),
            ..ToolInfo::new("revolve", "Revolve")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Angle".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.85, 0.16, 0.16, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        self.hovered_face = None;
        if let Some(f) = Self::selected_face(vp) {
            self.take_face(vp, f);
        }
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
        self.hovered_face = None;
        self.reset();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.face.is_none() {
            self.hovered_face = vp.pick_face(ctx.screen.0, ctx.screen.1).map(|f| f.1);
            vp.set_hover(self.hovered_face.map(|f| EntityRef::Face(MeshId::LOOSE, f)));
            return;
        }
        self.hover = Some(self.on_plane(ctx.world));
        vp.update();
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.face.is_none() {
            let picked = vp.pick_face(ctx.screen.0, ctx.screen.1).map(|f| f.1);
            match picked.or(self.hovered_face) {
                Some(f) => self.take_face(vp, f),
                None => vp.flash_status(&tr("Revolve: click the face to turn", &[]), 2500),
            }
            return;
        }
        if self.axis.is_some() {
            self.commit(vp, self.angle);
            return;
        }
        let p = self.on_plane(ctx.world);
        match self.axis_start {
            None => {
                if let Some(axis) = self.edge_axis(vp, ctx) {
                    self.set_axis(vp, axis);
                } else {
                    self.axis_start = Some(p);
                }
            }
            Some(a) => {
                let d = p - a;
                if d.length() > 1e-9 {
                    self.set_axis(vp, (a, d.unit_or_zero()));
                }
            }
        }
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: true,
            segments: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        match *value {
            ToolValue::Segments(n) => {
                if n < 3 {
                    return false;
                }
                self.sides = n;
                vp.flash_status(&tr("Revolve: {n} sides", &[("n", &n)]), 2500);
                self.rebuild_preview(vp);
                true
            }
            ToolValue::Num(deg) => {
                if !revolve_angle_ok(deg) {
                    vp.flash_status(&refusal_text(RevolveRefusal::Angle), 4000);
                    return true;
                }
                self.angle = deg;
                if self.axis.is_some() {
                    self.commit(vp, deg);
                } else {
                    vp.flash_status(&tr("Revolve angle: {deg}°", &[("deg", &deg)]), 2500);
                }
                true
            }
            _ => false,
        }
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
        vp.update();
    }

    fn drag_plane(&self, _vp: &Viewport) -> Option<Plane> {
        self.plane
    }

    fn start_point(&self) -> Option<DVec3> {
        self.axis_start
    }

    fn phase(&self) -> &'static str {
        if self.face.is_none() {
            "idle"
        } else if self.axis.is_some() {
            "angle"
        } else if self.axis_start.is_some() {
            "axis2"
        } else {
            "axis"
        }
    }

    fn busy(&self) -> bool {
        self.face.is_some()
    }

    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        if let (Some(a), Some(h)) = (self.axis_start, self.hover) {
            return vec![(a, h)];
        }
        let (Some((o, d)), Some(pf)) = (self.axis, self.profile(vp)) else {
            return Vec::new();
        };
        // the axis, as long as the face reaches along it, and a bit more
        let along = pf.vertices.iter().map(|&v| (v - o).dot(d));
        let (lo, hi) = along.fold((0.0f64, 0.0f64), |(a, b), t| (a.min(t), b.max(t)));
        let pad = (hi - lo).max(1e-3) * 0.25;
        vec![(o + d * (lo - pad), o + d * (hi + pad))]
    }

    fn preview_faces(&self) -> Vec<PreviewFace> {
        self.preview.clone()
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (o, _) = self.axis?;
        let text = tr(
            "{deg}° · {n} sides",
            &[("deg", &format!("{:.1}", self.angle)), ("n", &self.sides)],
        );
        Some((text, Some(self.hover.unwrap_or(o))))
    }
}
