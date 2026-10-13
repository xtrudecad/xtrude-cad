// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hole tool: drill a screw hole into a solid. Hover a face of a solid
//! (the centre snaps as points do) and click: the hole is cut square to
//! the face, into the material — one undo step. What the hole is (bore,
//! depth, head) is kept between uses: type `M3` (normal fit) or `M3c`
//! (close fit) for an ISO 273 clearance hole, a diameter, or
//! `diameter;depth` (a depth of 0 is through); Tab opens the whole spec
//! as a form, the head included. A hole off a solid, or one that would
//! come out the far side when it is not through, is refused with the
//! reason.

use std::cell::Cell;

use glam::DVec3;
use xtrude_core::hole::{clearance, Depth, Fit, Head, HoleSpec};
use xtrude_core::FaceId;
use xtrude_model::entity::EntityRef;
use xtrude_model::history::FnCommand;
use xtrude_model::hole::{apply_hole, plan_hole, HoleSite, HoleSolid};
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_len_fine;

use super::{Rgba, Tool, ToolContext, ToolInfo, ToolValue};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::ValueSpec;
use crate::viewport::{Viewport, ViewportEvent};

thread_local! {
    /// The hole last set, shared across activations.
    static SPEC: Cell<HoleSpec> = Cell::new(HoleSpec::default());
}

pub fn hole_spec() -> HoleSpec {
    SPEC.with(|c| c.get())
}

pub fn set_hole_spec(spec: HoleSpec) {
    SPEC.with(|c| c.set(spec));
}

/// The hole in words: `Ø3.4 mm, through, countersunk`.
pub fn describe(spec: &HoleSpec) -> String {
    let depth = match spec.depth {
        Depth::Through => tr("through", &[]),
        Depth::Blind(d) => tr("{d} deep", &[("d", &fmt_len_fine(d))]),
    };
    let head = match spec.head {
        Head::None => String::new(),
        Head::Countersink { diameter, .. } => {
            tr(", countersunk Ø{d}", &[("d", &fmt_len_fine(diameter))])
        }
        Head::Counterbore { diameter, depth } => tr(
            ", counterbored Ø{d} × {h}",
            &[("d", &fmt_len_fine(diameter)), ("h", &fmt_len_fine(depth))],
        ),
    };
    tr(
        "Ø{d}, {depth}{head}",
        &[
            ("d", &fmt_len_fine(spec.diameter)),
            ("depth", &depth),
            ("head", &head),
        ],
    )
}

#[derive(Default)]
pub struct HoleTool {
    /// The face under the cursor and the (snapped) point on it.
    pub hover: Option<(FaceId, DVec3)>,
    /// The solid under the cursor, for one mesh state.
    solid: Option<(FaceId, u64, Result<HoleSolid, String>)>,
    /// Where the hovered hole goes, or why it cannot.
    site: Option<Result<HoleSite, String>>,
}

impl HoleTool {
    pub fn new() -> HoleTool {
        HoleTool::default()
    }

    fn reset(&mut self) {
        *self = HoleTool::new();
    }

    /// The active mesh's face under the cursor.
    fn hovered_face(vp: &mut Viewport, ctx: &ToolContext) -> Option<FaceId> {
        let (m, f) = vp.pick_face(ctx.screen.0, ctx.screen.1)?;
        (m == vp.scene.active_mesh_id() && vp.scene.mesh().has_face(f)).then_some(f)
    }

    /// Find where the hovered hole goes (the solid kept while the mesh
    /// and the face stay the same).
    fn resite(&mut self, vp: &Viewport) {
        let Some((face, at)) = self.hover else {
            self.site = None;
            return;
        };
        let mesh = vp.scene.mesh();
        let serial = mesh.mut_serial;
        if !matches!(&self.solid, Some((f, s, _)) if *f == face && *s == serial) {
            let solid = HoleSolid::of(mesh, face).map_err(|e| e.message());
            self.solid = Some((face, serial, solid));
        }
        let Some((_, _, solid)) = &self.solid else {
            return;
        };
        let spec = hole_spec();
        self.site = Some(match solid {
            Ok(s) => s
                .site(mesh, face, at)
                .and_then(|site| site.check(&spec).map(|()| site))
                .map_err(|e| e.message()),
            Err(why) => Err(why.clone()),
        });
    }

    fn drill(&mut self, vp: &mut Viewport, face: FaceId, at: DVec3) {
        let spec = hole_spec();
        let plan = match plan_hole(vp.scene.mesh(), face, at, &spec) {
            Ok(p) => p,
            Err(e) => {
                vp.flash_status(&e.message(), 4000);
                return;
            }
        };
        vp.run(FnCommand::new(
            "SnapshotMutation",
            move |s: &mut xtrude_model::scene::Scene| {
                apply_hole(s.mesh_mut(), &plan);
                s.version += 1;
                Ok(())
            },
        ));
        vp.scene.clear_selection();
        self.reset();
        vp.set_hover(None);
        vp.flash_status(
            &tr("Drilled a hole {hole}.", &[("hole", &describe(&spec))]),
            3000,
        );
        vp.update();
    }

    /// Take a new spec and say what it is now.
    fn set_spec(&mut self, vp: &mut Viewport, spec: HoleSpec) {
        set_hole_spec(spec);
        self.resite(vp);
        vp.flash_status(
            &tr("Hole {hole} — click a face.", &[("hole", &describe(&spec))]),
            3000,
        );
        vp.update();
    }
}

impl Tool for HoleTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("hole"),
            description: Some(
                "Drill a screw hole into a solid, square to the face you click; type M3 for its clearance hole, Tab for the depth and the head.",
            ),
            ..ToolInfo::new("hole", "Hole")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Diameter".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.13, 0.17, 0.23, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.flash_status(
            &tr(
                "Hole {hole} — click a face of a solid.",
                &[("hole", &describe(&hole_spec()))],
            ),
            4000,
        );
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(face) = Self::hovered_face(vp, ctx) else {
            vp.flash_status(&tr("Click a face of a solid.", &[]), 3000);
            return;
        };
        self.drill(vp, face, ctx.world);
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let face = Self::hovered_face(vp, ctx);
        self.hover = face.map(|f| (f, ctx.world));
        self.resite(vp);
        let mid = vp.scene.active_mesh_id();
        vp.set_hover(face.map(|f| EntityRef::Face(mid, f)));
        vp.update();
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        if key == Key::Tab {
            vp.outbox.events.push(ViewportEvent::PromptHoleSpec);
            return true;
        }
        false
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            comma_lists: true,
            screw_sizes: true,
            ..ValueSpec::default()
        }
    }

    /// `M3` / `M3c`: the clearance hole; a diameter; `diameter;depth`, a
    /// depth of 0 being through.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let mut spec = hole_spec();
        match *value {
            ToolValue::Screw { nominal, close } => {
                let fit = if close { Fit::Close } else { Fit::Normal };
                let Some(d) = clearance(nominal, fit) else {
                    vp.flash_status(
                        &tr(
                            "No clearance hole for M{m}: the table holds M2 to M8.",
                            &[("m", &nominal)],
                        ),
                        3000,
                    );
                    return false;
                };
                spec.diameter = d;
            }
            ToolValue::Num(d) if d > 0.0 => spec.diameter = d,
            ToolValue::Pair(d, depth) if d > 0.0 && depth >= 0.0 => {
                spec.diameter = d;
                spec.depth = if depth > 0.0 {
                    Depth::Blind(depth)
                } else {
                    Depth::Through
                };
            }
            _ => return false,
        }
        self.set_spec(vp, spec);
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.set_hover(None);
        vp.update();
    }

    /// The hole's circles and its outline along the axis.
    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let Some(Ok(site)) = &self.site else {
            return Vec::new();
        };
        let rings = site.rings(&hole_spec());
        let mut lines: Vec<(DVec3, DVec3)> = Vec::new();
        for ring in &rings {
            let n = ring.len();
            lines.extend((0..n).map(|i| (ring[i], ring[(i + 1) % n])));
        }
        // four lines down the side
        for pair in rings.windows(2) {
            let n = pair[0].len();
            for k in (0..n).step_by((n / 4).max(1)) {
                lines.push((pair[0][k], pair[1][k]));
            }
        }
        lines
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (_, at) = self.hover?;
        match self.site.as_ref()? {
            Ok(site) => Some((describe(&hole_spec()), Some(site.centre))),
            Err(why) => Some((why.clone(), Some(at))),
        }
    }
}
