// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Measure tool: measure distances and place construction guides.
//!
//! What the first click lands on decides the mode: an edge's (or a guide
//! line's) BODY pulls out a guide line parallel to it at the dragged
//! or typed offset; an axis does the same; a named
//! point leaves a guide POINT with its dashed segment back to the start;
//! free space just measures. Ctrl (tapped alone) cycles guide lines →
//! guide points → measure only. Esc cancels.

use glam::DVec3;
use xtrude_core::axes;
use xtrude_core::math::VecExt;
use xtrude_core::snap::SnapKind;
use xtrude_core::Axis;
use xtrude_model::commands::AddGuideCommand;
use xtrude_model::guide::Guide;
use xtrude_model::i18n::tr;
use xtrude_model::units::{fmt_len, fmt_len_fine};

use super::{Plane, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::{Cursor, Viewport};

/// What the Measure tool leaves behind; Ctrl walks them in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureMode {
    /// A guide LINE parallel to the edge clicked.
    Line,
    /// A guide POINT at the measured distance.
    Point,
    /// Nothing left behind.
    Measure,
}

impl MeasureMode {
    fn next(self) -> MeasureMode {
        match self {
            MeasureMode::Line => MeasureMode::Point,
            MeasureMode::Point => MeasureMode::Measure,
            MeasureMode::Measure => MeasureMode::Line,
        }
    }

    fn flash(self) -> &'static str {
        match self {
            MeasureMode::Line => "Guides: guide lines",
            MeasureMode::Point => "Guides: guide points",
            MeasureMode::Measure => "Guides: off — measure only",
        }
    }
}

/// A guide source: an edge's span (or a guide line's, or an axis).
#[derive(Clone, Copy, Debug)]
struct Source {
    a: DVec3,
    b: DVec3,
}

/// Snap kinds that make the first click a POINT the Measure tool measures from
/// (a midpoint or a spot on an edge is not one: it is not a real point of
/// the model).
fn is_point_kind(k: &SnapKind) -> bool {
    matches!(
        k,
        SnapKind::Endpoint
            | SnapKind::Origin
            | SnapKind::Intersection
            | SnapKind::ComponentOrigin
            | SnapKind::Center
    )
}

pub struct MeasureTool {
    pub start_point: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub work_plane: Option<Plane>,
    edge: Option<Source>,
    /// Started on a named point → guide point.
    from_point: bool,
    pub measured: Option<f64>,
    pub mode: MeasureMode,
}

impl Default for MeasureTool {
    fn default() -> Self {
        MeasureTool {
            start_point: None,
            hover_point: None,
            work_plane: None,
            edge: None,
            from_point: false,
            measured: None,
            mode: MeasureMode::Line,
        }
    }
}

impl MeasureTool {
    pub fn new() -> MeasureTool {
        MeasureTool::default()
    }

    /// The guide source's span, `None` in measure mode.
    pub fn source(&self) -> Option<(DVec3, DVec3)> {
        self.edge.map(|e| (e.a, e.b))
    }

    /// Set the guide source directly (tests drive the preview this way).
    pub fn set_source(&mut self, span: Option<(DVec3, DVec3)>) {
        self.edge = span.map(|(a, b)| Source { a, b });
    }

    /// Whether this mode leaves anything behind.
    fn guides(&self) -> bool {
        self.mode != MeasureMode::Measure
    }

    fn reset(&mut self) {
        self.start_point = None;
        self.edge = None;
        self.from_point = false;
        self.work_plane = None;
    }

    fn toggle_mode(&mut self, vp: &mut Viewport) {
        self.mode = self.mode.next();
        vp.flash_status(&tr(self.mode.flash(), &[]), 2000);
        vp.tool_slot.cursor_dirty = true; // the + appears or disappears now
        vp.update();
    }

    fn edge_dir(&self) -> DVec3 {
        let e = self.edge.expect("a source");
        (e.b - e.a).unit_or_zero()
    }

    /// Cursor offset perpendicular to the source edge (the guide's pull).
    fn guide_offset(&self, world: DVec3) -> Option<DVec3> {
        self.edge?;
        let start = self.start_point?;
        let d = self.edge_dir();
        let delta = world - start;
        Some(delta - d * delta.dot(d))
    }

    /// A guide POINT at `at`, with its guide segment back to `origin` when
    /// measured from a named point.
    fn place_guide_point(&self, vp: &mut Viewport, at: DVec3, origin: Option<DVec3>) {
        vp.execute(&mut AddGuideCommand::new(Guide::new(at, None, origin)));
        let d = (at - self.start_point.unwrap_or(at)).length();
        let msg = if origin.is_some() {
            tr(
                "Guide point at {d}, with its segment",
                &[("d", &fmt_len_fine(d))],
            )
        } else {
            tr("Guide point at {d}", &[("d", &fmt_len_fine(d))])
        };
        vp.flash_status(&msg, 3000);
    }

    fn place_guide(&self, vp: &mut Viewport, offset: DVec3) {
        let Some(start) = self.start_point else {
            return;
        };
        let guide = Guide::new(start + offset, Some(self.edge_dir()), None);
        vp.execute(&mut AddGuideCommand::new(guide));
        vp.flash_status(
            &tr("Guide at {d}", &[("d", &fmt_len_fine(offset.length()))]),
            3000,
        );
    }

    fn first_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (x, y) = ctx.screen;
        self.start_point = Some(ctx.world);
        let kind = ctx.snap.as_ref().map_or(SnapKind::None, |s| s.kind.clone());
        // pick_edge_any: a group's edge is a source too, read from outside
        // as a world pseudo-edge.
        let mut edge = vp.pick_edge_any(x, y).map(|p| Source { a: p.a, b: p.b });
        if edge.is_none() {
            // A guide LINE is a source too: grids of guides.
            if let Some(gid) = vp.pick_guide(x, y) {
                if let Some(g) = vp.scene.doc.guides.iter().find(|g| g.id == gid) {
                    if g.is_line() {
                        edge = Some(Source { a: g.a(), b: g.b() });
                    }
                }
            }
        }
        self.from_point = is_point_kind(&kind);
        if edge.is_none() && kind == SnapKind::OnAxis {
            if let Some(snap) = &ctx.snap {
                if let Some(name) = snap
                    .axis
                    .filter(|c| matches!(c, Axis::X | Axis::Y | Axis::Z))
                {
                    // The cue already found the axis and put the point ON it.
                    let axis = axes::axis(name);
                    edge = Some(Source {
                        a: DVec3::ZERO,
                        b: axis,
                    });
                    self.start_point = Some(snap.point);
                }
            }
        }
        if edge.is_none() && !self.from_point {
            // The model's own axes are sources too: the start is the foot
            // on the axis, so the offset is measured from the line itself.
            if let Some(name) = vp.pick_axis(x, y) {
                let axis = axes::axis(name);
                edge = Some(Source {
                    a: DVec3::ZERO,
                    b: axis,
                });
                self.start_point = Some(axis * ctx.world.dot(axis));
            }
        }
        let blocks = matches!(
            kind,
            SnapKind::Endpoint
                | SnapKind::Midpoint
                | SnapKind::Close
                | SnapKind::Origin
                | SnapKind::Intersection
        );
        self.edge = if self.mode == MeasureMode::Line && !blocks {
            edge
        } else {
            None
        };
    }

    /// The little + beside the cursor: this one will leave a guide.
    pub fn cursor_plus(&self) -> bool {
        self.guides()
    }
}

impl Tool for MeasureTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("measure"),
            shortcut: Some("T"),
            description: Some("Measure distances, or pull guide lines off edges to draw against."),
            ..ToolInfo::new("measure", "Measure")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Distance".into())
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            axis_source: true,
            ..SnapPrefs::axis_magnet(true)
        }
    }
    fn cursor(&self, _vp: &Viewport) -> Option<Cursor> {
        self.info().icon.map(|key| Cursor::Tool {
            key,
            plus: self.cursor_plus(),
        })
    }
    /// «Ctrl = guide lines / guide points / measure», the active one
    /// bracketed.
    fn status_clause(&self) -> String {
        let names = [
            (MeasureMode::Line, tr("guide lines", &[])),
            (MeasureMode::Point, tr("guide points", &[])),
            (MeasureMode::Measure, tr("measure", &[])),
        ];
        let parts: Vec<String> = names
            .into_iter()
            .map(|(m, n)| if m == self.mode { format!("[{n}]") } else { n })
            .collect();
        format!("Ctrl = {}", parts.join(" / "))
    }
    tool_any!();

    fn start_point(&self) -> Option<DVec3> {
        self.start_point
    }
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        Some(WorkPlane {
            plane: self.work_plane,
            lock: None,
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.work_plane = plane;
    }

    /// Picking the tool up starts in guide mode.
    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.mode = MeasureMode::Line;
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
    }

    /// Ctrl toggles on the RELEASE of a Ctrl pressed alone; on the press,
    /// Ctrl+Z switched the mode too.
    fn on_key(&mut self, _vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        key == Key::Control
    }

    fn on_key_release(&mut self, vp: &mut Viewport, key: Key) -> Option<bool> {
        if key != Key::Control {
            return Some(false);
        }
        if !vp.ctrl_tapped() {
            return Some(false); // Ctrl was part of a shortcut
        }
        self.toggle_mode(vp);
        Some(true)
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(start) = self.start_point else {
            self.first_click(vp, ctx);
            return;
        };
        let kind = ctx.snap.as_ref().map_or(SnapKind::None, |s| s.kind.clone());
        if self.mode == MeasureMode::Point {
            // A guide POINT at the measured spot — no source edge needed.
            self.place_guide_point(vp, ctx.world, None);
        } else if self.edge.is_some() {
            if let Some(off) = self.guide_offset(ctx.world) {
                self.place_guide(vp, off);
            }
        } else if self.mode == MeasureMode::Line
            && self.from_point
            && !is_point_kind(&kind)
            && (ctx.world - start).length() > 1e-9
        {
            // From a named point to a free spot: a guide point there, with
            // its segment back to the start. To another named point it
            // only measures.
            self.place_guide_point(vp, ctx.world, Some(start));
        } else {
            let dist = (ctx.world - start).length();
            self.measured = Some(dist);
            vp.flash_status(&tr("Distance: {d}", &[("d", &fmt_len_fine(dist))]), 4000);
        }
        self.reset();
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
        vp.update();
    }

    /// Typing a distance places the guide — line or point — exactly there.
    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        let ToolValue::Num(v) = *value else {
            return false;
        };
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return false;
        };
        if self.mode == MeasureMode::Point {
            let d = hover - start;
            if d.length() < 1e-9 {
                return false;
            }
            self.place_guide_point(vp, start + d.unit_or_zero() * v, None);
            self.reset();
            vp.update();
            return true;
        }
        if self.edge.is_none() {
            if self.mode == MeasureMode::Line && self.from_point {
                let d = hover - start;
                if d.length() < 1e-9 {
                    return false;
                }
                self.place_guide_point(vp, start + d.unit_or_zero() * v, Some(start));
                self.reset();
                vp.update();
                return true;
            }
            return false;
        }
        let Some(off) = self.guide_offset(hover).filter(|o| o.length() >= 1e-9) else {
            return false;
        };
        self.place_guide(vp, off.unit_or_zero() * v);
        self.reset();
        vp.update();
        true
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        if self.edge.is_some() {
            if let Some(off) = self.guide_offset(hover) {
                return vec![(start, start + off)];
            }
        }
        vec![(start, hover)]
    }

    /// The guide line the next click leaves, parallel to its source.
    fn guide_preview_lines(&self) -> Vec<(DVec3, DVec3)> {
        let (Some(start), Some(hover)) = (self.start_point, self.hover_point) else {
            return Vec::new();
        };
        if self.edge.is_none() {
            return Vec::new();
        }
        match self.guide_offset(hover) {
            Some(off) => vec![Guide::new(start + off, Some(self.edge_dir()), None).segment()],
            None => Vec::new(),
        }
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (start, hover) = (self.start_point?, self.hover_point?);
        let d = if self.edge.is_some() {
            self.guide_offset(hover).map_or(0.0, |o| o.length())
        } else {
            (hover - start).length()
        };
        Some((fmt_len(d), Some((start + hover) * 0.5)))
    }
}
