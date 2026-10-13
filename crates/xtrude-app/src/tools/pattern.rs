// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Linear and Circular Pattern: copies of the selection along a direction
//! or around an axis, by count and spacing.
//!
//! Linear Pattern picks the direction with two points, as Move does (the
//! axis locks hold); the distance between them is the spacing. Circular
//! Pattern picks the centre and axis with Rotate's protractor; the copies
//! go round the full turn, or the swept angle apart after a reference
//! arm. The value box takes `count;spacing` (the instances, the original
//! among them, and the step between two of them), `count` (the step kept),
//! or `count/span` and `count/` (the instances spread over the span). The
//! copies are stamped by Move's and Rotate's copy commands, in one undo
//! step; right after one, a typed entry lays the pattern again.

use glam::DVec3;
use xtrude_core::math::{MatExt, VecExt};
use xtrude_model::commands::rotation_matrix;
use xtrude_model::i18n::tr;
use xtrude_model::units::fmt_len;

use super::protractor::ProtractorBase;
use super::transform::{array_multiples, gather_targets, CmdMark, CopySet};
use super::{
    Plane, Rgba, SnapPrefs, Tool, ToolContext, ToolInfo, ToolValue, WorkPlane, MAGNET_DEG,
};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::value_box::{ArrayKind, PatternEntry, ValueSpec};
use crate::viewport::Viewport;

/// The instances a new pattern lays until a count is typed.
pub const DEFAULT_COUNT: u32 = 3;
/// The most instances a pattern lays.
pub const MAX_COUNT: u32 = 1000;
const FULL_TURN: f64 = 360.0;

/// The step between two instances when `count` of them spread over
/// `span`; a full turn closes on itself, so its last gap is the first.
fn spread_step(count: u32, span: f64, closes: bool) -> f64 {
    let gaps = if closes { count } else { count - 1 };
    span / gaps as f64
}

/// Whether `count` is a pattern's, and why not.
fn check_count(vp: &mut Viewport, count: u32) -> bool {
    if (2..=MAX_COUNT).contains(&count) {
        return true;
    }
    vp.flash_status(
        &tr("A pattern has 2 to {max} instances", &[("max", &MAX_COUNT)]),
        2500,
    );
    false
}

/// What the copies are: the copy set and its wireframe, gathered as Move
/// and Rotate do, or `None` (and the reason shown) when there is nothing
/// to copy.
fn gather(vp: &mut Viewport, ctx: &ToolContext) -> Option<(CopySet, Vec<(DVec3, DVec3)>)> {
    let (groups, _) = gather_targets(vp, ctx);
    let (set, segs) = CopySet::gather(vp, ctx, &groups);
    if set.is_empty() {
        vp.flash_status(
            &tr("Select (or click) the geometry to repeat first", &[]),
            2500,
        );
        return None;
    }
    Some((set, segs))
}

/// A pattern laid, for a retype right after it.
#[derive(Clone, Debug)]
struct LastPattern {
    mark: CmdMark,
    set: CopySet,
    count: u32,
    /// The step between two instances: a length along `dir` (linear), or
    /// degrees about the axis `dir` through `centre` (circular).
    step: f64,
    dir: DVec3,
    centre: DVec3,
}

impl LastPattern {
    /// The step a linear retype asks for: typed, or the one laid (with
    /// `/`, the span the last pattern covered, spread anew).
    fn linear_step(&self, p: &PatternEntry) -> f64 {
        match (p.step, p.spread) {
            (Some(s), false) => s,
            (Some(span), true) => spread_step(p.count, span, false),
            (None, false) => self.step,
            (None, true) => spread_step(p.count, self.step * (self.count - 1) as f64, false),
        }
    }
}

/// Replace the pattern `last` laid with `count` instances `step` apart,
/// built by `build`; whether it was replaced.
fn relay(
    vp: &mut Viewport,
    last: &mut Option<LastPattern>,
    count: u32,
    step: f64,
    build: impl Fn(&LastPattern, u32, f64) -> Option<Box<dyn xtrude_model::history::Command>>,
) -> bool {
    let Some(mut l) = last.clone() else {
        return false;
    };
    if !l.mark.is_last(vp) {
        *last = None;
        return false;
    }
    let Some(mut cmd) = build(&l, count, step) else {
        return false;
    };
    vp.undo();
    vp.execute(cmd.as_mut());
    l.mark = CmdMark::now(vp);
    l.count = count;
    l.step = step;
    *last = Some(l);
    flash_count(vp, count);
    vp.update();
    true
}

fn flash_count(vp: &mut Viewport, count: u32) {
    vp.flash_status(&tr("{n} instances", &[("n", &count)]), 2500);
}

// ---------------------------------------------------------------------------
// Linear Pattern

/// Linear Pattern: the selection repeated along a direction.
#[derive(Default)]
pub struct LinearPatternTool {
    pub start: Option<DVec3>,
    pub hover: Option<DVec3>,
    /// The instances the next pattern lays.
    pub count: u32,
    set: CopySet,
    base_segments: Vec<(DVec3, DVec3)>,
    last: Option<LastPattern>,
}

impl LinearPatternTool {
    pub fn new() -> LinearPatternTool {
        LinearPatternTool {
            count: DEFAULT_COUNT,
            ..LinearPatternTool::default()
        }
    }

    fn reset(&mut self) {
        self.start = None;
        self.hover = None;
        self.set = CopySet::default();
        self.base_segments.clear();
    }

    fn deltas(dir: DVec3, count: u32, step: f64) -> Vec<DVec3> {
        array_multiples(count - 1, ArrayKind::External)
            .into_iter()
            .map(|k| dir * (step * k))
            .collect()
    }

    fn build(
        l: &LastPattern,
        count: u32,
        step: f64,
    ) -> Option<Box<dyn xtrude_model::history::Command>> {
        l.set.translated(Self::deltas(l.dir, count, step))
    }

    /// Lay `count` instances `step` apart along `dir` (a unit vector).
    fn lay(&mut self, vp: &mut Viewport, dir: DVec3, count: u32, step: f64) {
        let mut last = LastPattern {
            mark: CmdMark::sentinel(),
            set: std::mem::take(&mut self.set),
            count,
            step,
            dir,
            centre: DVec3::ZERO,
        };
        self.reset();
        if let Some(mut cmd) = Self::build(&last, count, step) {
            vp.execute(cmd.as_mut());
            last.mark = CmdMark::now(vp);
            self.last = Some(last);
            flash_count(vp, count);
        }
        vp.update();
    }

    /// The picked direction (unit) and length, `None` before it has one.
    fn picked(&self) -> Option<(DVec3, f64)> {
        let d = self.hover? - self.start?;
        (d.length() > 1e-9).then(|| (d.unit_or_zero(), d.length()))
    }

    fn on_pattern(&mut self, vp: &mut Viewport, p: &PatternEntry) -> bool {
        if !check_count(vp, p.count) {
            return true;
        }
        if self.start.is_some() {
            let Some((dir, len)) = self.picked() else {
                return false;
            };
            let step = match (p.step, p.spread) {
                (Some(s), false) => s,
                (span, true) => spread_step(p.count, span.unwrap_or(len), false),
                (None, false) => len,
            };
            self.count = p.count;
            self.lay(vp, dir, p.count, step);
            return true;
        }
        if let Some(l) = &self.last {
            let step = l.linear_step(p);
            self.count = p.count;
            return relay(vp, &mut self.last, p.count, step, Self::build);
        }
        self.count = p.count;
        flash_count(vp, p.count);
        true
    }
}

impl Tool for LinearPatternTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("pattern_linear"),
            description: Some("Repeat the selection along a direction, by count and spacing."),
            ..ToolInfo::new("pattern_linear", "Linear Pattern")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Count;Spacing".into())
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            magnetic_axis_deg: Some(15.0),
            screen_axis_px: Some(9.0),
            prefers_vertical_drag: true,
            ..SnapPrefs::default()
        }
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.last = None;
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.last = None;
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if self.start.is_none() {
            let Some((set, segs)) = gather(vp, ctx) else {
                return;
            };
            self.last = None;
            self.set = set;
            self.base_segments = segs;
            self.start = Some(ctx.world);
            self.hover = Some(ctx.world);
            return;
        }
        self.hover = Some(ctx.world);
        if let Some((dir, len)) = self.picked() {
            self.lay(vp, dir, self.count, len);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.hover = Some(ctx.world);
        vp.update();
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            pattern: true,
            comma_lists: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        match value {
            ToolValue::Pattern(p) => self.on_pattern(vp, p),
            _ => false,
        }
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        self.last = None;
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let (Some(s), Some(h)) = (self.start, self.hover) else {
            return Vec::new();
        };
        let mut segs = vec![(s, h)];
        let d = h - s;
        for k in 1..self.count {
            let off = d * k as f64;
            segs.extend(self.base_segments.iter().map(|(a, b)| (*a + off, *b + off)));
        }
        segs
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (s, h) = (self.start?, self.hover?);
        let text = format!("{} × {}", self.count, fmt_len((h - s).length()));
        Some((text, Some((s + h) * 0.5)))
    }

    fn start_point(&self) -> Option<DVec3> {
        self.start
    }
}

// ---------------------------------------------------------------------------
// Circular Pattern

/// Circular Pattern: the selection repeated around an axis.
#[derive(Default)]
pub struct CircularPatternTool {
    pub p: ProtractorBase,
    /// The instances the next pattern lays.
    pub count: u32,
    set: CopySet,
    base_segments: Vec<(DVec3, DVec3)>,
    last: Option<LastPattern>,
}

impl CircularPatternTool {
    pub fn new() -> CircularPatternTool {
        CircularPatternTool {
            count: DEFAULT_COUNT,
            ..CircularPatternTool::default()
        }
    }

    fn reset(&mut self) {
        self.p.reset();
        self.set = CopySet::default();
        self.base_segments.clear();
    }

    fn degs(count: u32, step: f64) -> Vec<f64> {
        array_multiples(count - 1, ArrayKind::External)
            .into_iter()
            .map(|k| step * k)
            .collect()
    }

    fn build(
        l: &LastPattern,
        count: u32,
        step: f64,
    ) -> Option<Box<dyn xtrude_model::history::Command>> {
        l.set.rotated(l.centre, l.dir, Self::degs(count, step))
    }

    /// The step the preview shows: the swept angle after a reference arm,
    /// else the full turn shared.
    fn preview_step(&self) -> Option<f64> {
        self.p.start_point?;
        if self.p.ref_point.is_some() {
            return self.p.commit_deg(self.p.hover_point?);
        }
        Some(FULL_TURN / self.count as f64)
    }

    fn lay(&mut self, vp: &mut Viewport, count: u32, step: f64) {
        let Some(centre) = self.p.start_point else {
            return;
        };
        let mut last = LastPattern {
            mark: CmdMark::sentinel(),
            set: std::mem::take(&mut self.set),
            count,
            step,
            dir: self.p.axis(),
            centre,
        };
        self.reset();
        if step.abs() > 1e-9 {
            if let Some(mut cmd) = Self::build(&last, count, step) {
                vp.execute(cmd.as_mut());
                last.mark = CmdMark::now(vp);
                self.last = Some(last);
                flash_count(vp, count);
            }
        }
        vp.update();
    }

    fn on_pattern(&mut self, vp: &mut Viewport, p: &PatternEntry) -> bool {
        if !check_count(vp, p.count) {
            return true;
        }
        self.count = p.count;
        let step = match (p.step, p.spread) {
            (Some(s), false) => s,
            (Some(span), true) => spread_step(p.count, span, span.abs() >= FULL_TURN - 1e-9),
            (None, _) => FULL_TURN / p.count as f64,
        };
        if self.p.start_point.is_some() {
            self.lay(vp, p.count, step);
            return true;
        }
        if self.last.is_some() {
            return relay(vp, &mut self.last, p.count, step, Self::build);
        }
        flash_count(vp, p.count);
        true
    }
}

impl Tool for CircularPatternTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("pattern_circular"),
            description: Some("Repeat the selection around an axis, by count and angle."),
            ..ToolInfo::new("pattern_circular", "Circular Pattern")
        };
        &INFO
    }
    fn value_box_caption(&self) -> Option<String> {
        Some("Count;Angle".into())
    }
    fn wireframe_color(&self) -> Option<Rgba> {
        Some(self.p.wireframe_color())
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.last = None;
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.last = None;
        self.p.axis_pick = None;
        self.p.hover_point = None;
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        self.p.arrow_key(vp, key)
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(s) = self.p.start_point else {
            let Some((set, segs)) = gather(vp, ctx) else {
                return;
            };
            self.last = None;
            self.set = set;
            self.base_segments = segs;
            self.p.start_point = Some(ctx.world);
            self.p.axis_drag_armed = true;
            return;
        };
        if self.p.ref_point.is_none() {
            if (ctx.world - s).length() < 1e-6 {
                return;
            }
            self.p.ref_point = Some(ctx.world);
            return;
        }
        self.p.note_snap(ctx);
        if let Some(deg) = self.p.commit_deg(ctx.world) {
            self.lay(vp, self.count, deg);
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.p.hover_point = Some(ctx.world);
        self.p.track_axis_drag(vp);
        self.p.infer_plane(vp, ctx);
        self.p.update_screen_metrics(vp, ctx);
        self.p.note_snap(ctx);
        vp.update();
    }

    fn on_release(&mut self, vp: &mut Viewport) {
        if self.p.release_axis_drag(vp) {
            vp.flash_status(&tr("Pattern axis set along the drag", &[]), 2500);
        }
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: true,
            pattern: true,
            comma_lists: true,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, vp: &mut Viewport, value: &ToolValue) -> bool {
        match value {
            ToolValue::Pattern(p) => self.on_pattern(vp, p),
            _ => false,
        }
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        self.last = None;
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let mut segs = self.p.preview();
        let (Some(s), Some(step)) = (self.p.start_point, self.preview_step()) else {
            return segs;
        };
        for deg in Self::degs(self.count, step) {
            let m = rotation_matrix(s, self.p.axis(), deg);
            segs.extend(
                self.base_segments
                    .iter()
                    .map(|(a, b)| (m.project_point(*a), m.project_point(*b))),
            );
        }
        segs
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let step = self.preview_step()?;
        let at = self.p.hover_point.or(self.p.start_point);
        Some((format!("{} × {step:.1}°", self.count), at))
    }

    fn start_point(&self) -> Option<DVec3> {
        self.p.start_point
    }
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        Some(WorkPlane {
            plane: self.p.work_plane,
            lock: None,
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.p.work_plane = plane;
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            magnetic_axis_deg: Some(MAGNET_DEG),
            radial_arm: true,
            ..SnapPrefs::default()
        }
    }
}
