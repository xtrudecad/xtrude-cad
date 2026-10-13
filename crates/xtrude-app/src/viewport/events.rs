// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Input: mouse, wheel and keys, the Esc cascade, the value box's Enter.

use glam::DVec3;
use xtrude_core::camera::{OrbitCamera, MAX_DISTANCE, MIN_DISTANCE};
use xtrude_core::snap::LinearMode;
use xtrude_core::Axis;
use xtrude_model::i18n::tr;

use super::input_state::is_modifier_key;
use super::nav_style::{NavAction, Trigger};
use super::navigation::{CameraDrag, DragBy, RightPress, RubberBand};
use super::{Cursor, NavMode, Viewport};
use crate::input::{Button, Key, KeyEvent, Modifiers, MouseEvent, ScreenPt};
use crate::tools::{selection_mode, ToolValue};
use crate::value_box::{is_chord, Entry, ValueSpec};

/// Below this many pixels of drag, a press/release is a click, not a box.
pub const BOX_DRAG_THRESHOLD_PX: f64 = 4.0;
/// A right press and release closer than this many pixels, and within
/// [`CLICK_MAX_MS`], is a click: it opens the context menu.
pub const CLICK_MAX_DIST_PX: f64 = 6.0;
/// See [`CLICK_MAX_DIST_PX`].
pub const CLICK_MAX_MS: u64 = 800;
/// The usual desktop double-click interval (ms), for the triple click.
pub const DOUBLE_CLICK_MS: u64 = 400;

fn ipos(e: &MouseEvent) -> (i64, i64) {
    (e.x.round() as i64, e.y.round() as i64)
}

/// The camera's pose, rounded, to tell whether it moved since a wheel notch.
fn pose_key(c: &OrbitCamera) -> (i64, i64, i64, i64, i64, i64) {
    let r = |v: f64, n: i32| (v * 10f64.powi(n)).round() as i64;
    (
        r(c.yaw, 9),
        r(c.pitch, 9),
        r(c.distance, 6),
        r(c.target.x, 6),
        r(c.target.y, 6),
        r(c.target.z, 6),
    )
}

/// What the last wheel step zoomed toward. The next steps of the same
/// gesture (same camera, cursor within 24 px, model unchanged) reuse it
/// instead of picking again.
#[derive(Clone, Copy, Debug)]
pub(super) struct ZoomFocus {
    /// The camera after that step, rounded.
    pose: (i64, i64, i64, i64, i64, i64),
    at: ScreenPt,
    scene_version: u64,
    point: Option<DVec3>,
}

impl Viewport {
    // ---- mouse ---------------------------------------------------------------------------
    pub fn mouse_press(&mut self, ev: &MouseEvent) {
        self.input.held_modifiers = ev.modifiers;
        self.release_linear_mode();
        let Some(button) = ev.button else { return };
        self.nav.release_lost_unless_held(ev);
        // Alt + click is not a tap
        self.input.alt_cycle_on_release = false;
        if self.chord_press(ev) {
            return;
        }
        if button == Button::Right {
            self.nav.right_press = Some(RightPress {
                at: ScreenPt::new(ev.x, ev.y),
                time_ms: ev.time_ms,
            });
        }
        let style = self.prefs.nav_style;
        if let Some(action) = style.action(Trigger::Drag(button), ev.modifiers) {
            self.start_camera_drag(ev, DragBy::Button(button), action);
            return;
        }
        if button == Button::Left {
            if let Some(nav) = self.nav.mode {
                if nav == NavMode::ZoomWindow {
                    self.nav.zoom_box = Some(RubberBand::at(ScreenPt::new(ev.x, ev.y)));
                    return;
                }
                let action = if nav == NavMode::Zoom {
                    NavAction::Zoom
                } else if nav == NavMode::Pan || ev.modifiers.shift {
                    NavAction::Pan
                } else {
                    NavAction::Orbit
                };
                self.nav.camera_drag = Some(CameraDrag {
                    last: ipos(ev),
                    by: DragBy::Button(Button::Left),
                    action,
                });
                self.nav.orbit_pivot =
                    (nav == NavMode::Orbit).then(|| self.orbit_pivot_at(ev.x, ev.y));
                self.nav.pan_depth = if action == NavAction::Pan {
                    let p = self.orbit_pivot_at(ev.x, ev.y);
                    Some(self.depth_of(p))
                } else {
                    None
                };
                return;
            }
            if self.active_tool().is_none() {
                return;
            }
            if self.is_triple_click(ev) {
                self.input.last_double = None;
                if let Some(ctx) = self.build_ctx(ev.x, ev.y, ev.modifiers) {
                    self.with_tool(|t, vp| t.on_triple_click(vp, &ctx));
                    self.update();
                }
                return;
            }
            if self.active_tool().is_some_and(|t| t.info().box_select) {
                self.select_box = Some(RubberBand::at(ScreenPt::new(ev.x, ev.y)));
                return;
            }
            self.dispatch_tool_click(ev, false);
        }
    }

    /// Start a camera drag of the style's: orbit about the point under
    /// the pointer, or pan or zoom at its depth.
    fn start_camera_drag(&mut self, ev: &MouseEvent, by: DragBy, action: NavAction) {
        if by == DragBy::Move {
            // the Alt held for an Alt + move drag is not a tap
            self.input.alt_clean = false;
            self.input.alt_cycle_on_release = false;
        }
        self.nav.camera_drag = Some(CameraDrag {
            last: ipos(ev),
            by,
            action,
        });
        let pivot = self.orbit_pivot_at(ev.x, ev.y);
        self.nav.orbit_pivot = Some(pivot);
        self.nav.pan_depth = Some(self.depth_of(pivot));
        self.outbox.cursor = Some(match action {
            NavAction::Orbit => Cursor::Orbit,
            NavAction::Pan => Cursor::Pan,
            NavAction::Zoom => Cursor::Zoom,
        });
    }

    /// A left press with the right button held, or a right press with
    /// the left held, in a style that pans with the chord: the chord
    /// starts, and what the first button began stops — a right press is
    /// no longer a click; a left press drops its box, or the tool that
    /// took it gets its release now.
    fn chord_press(&mut self, ev: &MouseEvent) -> bool {
        let other_held = match ev.button {
            Some(Button::Left) => ev.right,
            Some(Button::Right) => ev.left,
            _ => false,
        };
        if !other_held {
            return false;
        }
        let Some(action) = self
            .prefs
            .nav_style
            .action(Trigger::LeftRightChord, ev.modifiers)
        else {
            return false;
        };
        self.nav.right_press = None;
        let left_drag = self
            .nav
            .camera_drag
            .is_some_and(|d| d.by == DragBy::Button(Button::Left));
        if ev.button == Some(Button::Right)
            && !left_drag
            && self.nav.mode.is_none()
            && self.nav.zoom_box.is_none()
            && self.select_box.take().is_none()
            && self.active_tool().is_some()
        {
            self.with_tool(|t, vp| t.on_release(vp));
        }
        self.select_box = None;
        self.nav.zoom_box = None;
        self.start_camera_drag(ev, DragBy::Chord, action);
        self.update();
        true
    }

    fn is_triple_click(&self, ev: &MouseEvent) -> bool {
        let Some((t, pos)) = self.input.last_double else {
            return false;
        };
        ev.time_ms.saturating_sub(t) <= DOUBLE_CLICK_MS
            && ((ev.x - pos.0).abs() + (ev.y - pos.1).abs()) < 8.0
    }

    /// Forward a (double-)click to the active tool, then the shared
    /// follow-ups: lock the chain to a clicked face's plane, clear the
    /// value box, release a spent axis lock.
    fn dispatch_tool_click(&mut self, ev: &MouseEvent, double: bool) {
        let had_start = self.tool_start_point().is_some();
        let had_plane = self
            .active_tool()
            .and_then(|t| t.work_plane())
            .is_some_and(|w| w.plane.is_some());
        let face_at_click = if !had_start && !had_plane {
            self.pick_face_placement(ev.x, ev.y)
        } else {
            None
        };
        let Some(ctx) = self.build_ctx(ev.x, ev.y, ev.modifiers) else {
            return;
        };
        let before = self.history.undo_len();
        self.with_tool(|t, vp| {
            if double {
                t.on_double_click(vp, &ctx)
            } else {
                t.on_click(vp, &ctx)
            }
        });
        let drew = self.history.undo_len() > before;
        let now_start = self.tool_start_point();
        if !had_start && now_start.is_some() {
            if let Some(hit) = face_at_click {
                let takes = self
                    .active_tool()
                    .and_then(|t| t.work_plane())
                    .is_some_and(|w| w.plane.is_none());
                if takes {
                    let plane = self.face_plane_world(&hit);
                    if let Some(t) = self.active_tool_mut() {
                        t.set_work_plane(Some(plane));
                    }
                }
            }
        }
        self.set_value_buffer("");
        if let Some(err) = self.history.last_error.clone() {
            let msg = tr("Operation failed and was undone: {err}", &[("err", &err)]);
            self.flash_status(&msg, 8000);
        }
        self.release_axis_lock_after_operation(drew);
        if drew {
            self.reassert_tool_cursor();
        }
        self.update();
    }

    fn reassert_tool_cursor(&mut self) {
        if self.active_tool().is_none() || self.nav.mode.is_some() {
            return;
        }
        self.apply_tool_cursor();
    }

    pub fn mouse_double_click(&mut self, ev: &MouseEvent) {
        self.input.held_modifiers = ev.modifiers;
        self.nav.release_lost_unless_held(ev);
        if self.chord_press(ev) {
            return;
        }
        if ev.button != Some(Button::Left)
            || self.active_tool().is_none()
            || self.nav.mode.is_some()
        {
            self.mouse_press(ev);
            return;
        }
        self.input.last_double = Some((ev.time_ms, (ev.x, ev.y)));
        if self.active_tool().is_some_and(|t| t.info().box_select) {
            if let Some(ctx) = self.build_ctx(ev.x, ev.y, ev.modifiers) {
                self.with_tool(|t, vp| t.on_double_click(vp, &ctx));
                self.update();
            }
            return;
        }
        self.dispatch_tool_click(ev, true);
    }

    pub fn mouse_move(&mut self, ev: &MouseEvent) {
        self.input.held_modifiers = ev.modifiers;
        let alt = ev.modifiers.alt;
        if alt != self.input.alt_down {
            self.apply_alt_cursor(alt);
        }
        let any_button = ev.left || ev.middle || ev.right;
        self.nav.release_lost_unless_held(ev);
        let style = self.prefs.nav_style;
        if let Some(rp) = self.nav.right_press {
            if rp.at.distance(ScreenPt::new(ev.x, ev.y)) > CLICK_MAX_DIST_PX {
                self.nav.right_press = None;
            }
        }
        if let Some(drag) = self.nav.camera_drag {
            let over = match drag.by {
                DragBy::Move => {
                    any_button || style.action(Trigger::Move, ev.modifiers) != Some(drag.action)
                }
                _ => !any_button,
            };
            if over {
                self.end_camera_drag();
            }
        }
        if self.starts_move_drag(ev) {
            if let Some(action) = style.action(Trigger::Move, ev.modifiers) {
                self.start_camera_drag(ev, DragBy::Move, action);
                self.update();
                return;
            }
        }
        if let Some(drag) = self.nav.camera_drag {
            // a right drag waits until it is no longer a click (the
            // pointer left CLICK_MAX_DIST_PX)
            if drag.by == DragBy::Button(Button::Right) && self.nav.right_press.is_some() {
                return;
            }
            let p = ipos(ev);
            let (dx, dy) = ((p.0 - drag.last.0) as f64, (p.1 - drag.last.1) as f64);
            self.nav.camera_drag = Some(CameraDrag { last: p, ..drag });
            let h = self.height() as i32;
            if self.nav.mode == Some(NavMode::Zoom) || drag.action == NavAction::Zoom {
                self.camera.zoom(-dy * 0.035);
            } else if drag.action == NavAction::Pan {
                self.camera.pan(dx, dy, h, self.nav.pan_depth);
            } else {
                let ody = if self.prefs.invert_orbit_y { -dy } else { dy };
                match self.nav.orbit_pivot {
                    Some(pivot) => self.camera.orbit_about(pivot, dx, ody, h),
                    None => self.camera.orbit(dx, ody, h),
                }
            }
            self.update();
            return;
        }
        if let Some(band) = &mut self.select_box {
            band.cur = ScreenPt::new(ev.x, ev.y);
            self.update();
            return;
        }
        if let Some(band) = &mut self.nav.zoom_box {
            band.cur = ScreenPt::new(ev.x, ev.y);
            self.update();
            return;
        }
        self.process_hover(ev.x, ev.y, ev.modifiers);
    }

    /// Whether `ev`, a move, starts a camera drag of the style's with no
    /// button held (Touchpad): none under way, no box being dragged.
    pub(super) fn starts_move_drag(&self, ev: &MouseEvent) -> bool {
        !(ev.left || ev.middle || ev.right)
            && self.nav.camera_drag.is_none()
            && self.select_box.is_none()
            && self.nav.zoom_box.is_none()
            && self
                .prefs
                .nav_style
                .action(Trigger::Move, ev.modifiers)
                .is_some()
    }

    fn end_camera_drag(&mut self) {
        self.nav.end_drag();
        if self.nav.mode.is_none() {
            self.apply_tool_cursor();
        }
    }

    /// How far `point` lies in front of the eye along the view.
    pub fn depth_of(&self, point: DVec3) -> f64 {
        (point - self.camera.eye()).dot(self.camera.forward())
    }

    /// A hover at `(x, y)`: the hovered edge, the centre reference, the
    /// acquisitions, the snap, and the tool's `on_hover`.
    pub fn process_hover(&mut self, x: f64, y: f64, modifiers: Modifiers) {
        if self.nav.camera_drag.is_some() || self.select_box.is_some() {
            return;
        }
        self.input.held_modifiers = modifiers;
        self.input.last_mouse_pos = Some(ScreenPt { x, y });
        let (uses_snap, group_edges) = self.active_tool().map_or((false, false), |t| {
            let prefs = t.snap_prefs();
            (prefs.uses_snap, prefs.hover_group_edges)
        });
        self.inference.hover_edge = if self.active_tool().is_some() && (uses_snap || group_edges) {
            self.pick_edge_any(x, y)
        } else {
            self.pick_edge(x, y)
        };
        if self.active_tool().is_some() && uses_snap {
            self.inference.hover_center = self.update_center_ref(x, y);
        }
        let drawing = self.tool_start_point().is_some();
        // What is in motion lends no corner and no edge.
        let moving = if drawing { self.in_motion() } else { None };
        if drawing && uses_snap {
            let corner = match &moving {
                Some(m) => self.pick_still_vertex(x, y, m),
                None => self.pick_vertex(x, y),
            };
            if let Some(corner) = corner {
                self.inference.acquired_point = Some(corner);
            }
        }
        if !drawing {
            self.inference.acquired_edge = None;
            self.inference.acquired_face_normal = None;
        } else {
            if let Some(e) = &self.inference.hover_edge {
                if moving.as_ref().is_none_or(|m| !m.has_edge(e)) {
                    self.inference.acquired_edge = Some((e.a, e.b));
                }
            }
            if let Some(hit) = self.pick_face_placement(x, y) {
                self.inference.acquired_face_normal = Some(self.face_plane_world(&hit).1);
            }
        }
        if self.active_tool().is_none() {
            return;
        }
        let Some(ctx) = self.build_ctx(x, y, modifiers) else {
            return;
        };
        self.inference.last_snap = ctx.snap.clone();
        if uses_snap {
            let mut point = None;
            if let Some(s) = &ctx.snap {
                if super::snapping::ENCOURAGING_KINDS.contains(&s.kind.as_str()) {
                    point = Some(s.point);
                }
            }
            if let Some(c) = &self.inference.hover_center {
                if point.is_none() || matches!(c.key, super::snapping::CenterKey::Edge(_)) {
                    point = Some(c.center);
                }
            }
            self.dwell_on(point);
        }
        self.with_tool(|t, vp| t.on_hover(vp, &ctx));
        self.emit_measurement();
        self.update();
    }

    pub fn mouse_release(&mut self, ev: &MouseEvent) {
        self.input.held_modifiers = ev.modifiers;
        if ev.button.is_some() && ev.button == self.nav.swallow_release {
            self.nav.swallow_release = None;
            return;
        }
        if ev.button == Some(Button::Middle) {
            self.end_camera_drag();
            return;
        }
        let chord = self.nav.camera_drag.is_some_and(|d| d.by == DragBy::Chord);
        if chord {
            self.end_camera_drag();
            self.nav.swallow_release = match ev.button {
                Some(Button::Left) => Some(Button::Right),
                _ => Some(Button::Left),
            };
            return;
        }
        if ev.button == Some(Button::Right) {
            let right_drag = self
                .nav
                .camera_drag
                .is_some_and(|d| d.by == DragBy::Button(Button::Right));
            if right_drag {
                self.end_camera_drag();
            }
            if let Some(rp) = self.nav.right_press.take() {
                if ev.time_ms.saturating_sub(rp.time_ms) <= CLICK_MAX_MS {
                    self.outbox.events.push(super::ViewportEvent::ContextMenu);
                }
            }
            return;
        }
        let left = ev.button == Some(Button::Left);
        if left && self.nav.zoom_box.is_some() {
            if let Some(band) = self.nav.zoom_box.take() {
                let (s, e) = band.corners();
                self.zoom_to_box(s, e);
            }
            self.update();
            return;
        }
        if left && self.nav.mode.is_some() {
            self.nav.end_drag();
            return;
        }
        if left && self.active_tool().is_some() && self.select_box.is_none() {
            let before = self.history.undo_len();
            self.with_tool(|t, vp| t.on_release(vp));
            self.release_axis_lock_after_operation(false);
            if self.history.undo_len() > before {
                self.reassert_tool_cursor();
            }
        }
        if left && self.select_box.is_some() {
            let Some(RubberBand { start, .. }) = self.select_box.take() else {
                self.update();
                return;
            };
            if self.active_tool().is_none() {
                self.update();
                return;
            }
            let (dx, dy) = (ev.x - start.x, ev.y - start.y);
            let mode = selection_mode(ev.modifiers);
            if dx.hypot(dy) < BOX_DRAG_THRESHOLD_PX {
                if let Some(ctx) = self.build_ctx(ev.x, ev.y, ev.modifiers) {
                    self.with_tool(|t, vp| t.on_click(vp, &ctx));
                }
            } else {
                let rect = (
                    start.x.min(ev.x),
                    start.y.min(ev.y),
                    start.x.max(ev.x),
                    start.y.max(ev.y),
                );
                let crossing = dx < 0.0;
                self.with_tool(|t, vp| t.on_box_select(vp, rect, crossing, mode));
            }
            self.update();
        }
    }

    pub fn zoom_box(&self) -> Option<((f64, f64), (f64, f64))> {
        self.nav.zoom_box.map(|b| b.corners())
    }

    /// Zoom Window: the dragged rectangle fills the view.
    pub fn zoom_to_box(&mut self, start: (f64, f64), end: (f64, f64)) {
        let (bw, bh) = ((end.0 - start.0).abs(), (end.1 - start.1).abs());
        if bw < 6.0 || bh < 6.0 {
            return;
        }
        let (cx, cy) = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
        if let Some(focus) = self.world_under_cursor(cx, cy) {
            self.camera.target = focus;
        }
        let frac = (bw / self.width().max(1.0))
            .max(bh / self.height().max(1.0))
            .max(0.04);
        self.camera.distance = (self.camera.distance * frac).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.update();
    }

    /// The model's size, for the zoom step.
    fn model_diag(&mut self) -> f64 {
        match self.scene.bounds() {
            Some((lo, hi)) => (hi - lo).length(),
            None => 0.0,
        }
    }

    /// The wheel: zoom toward the point under the cursor. `delta_y` in
    /// the common wheel-delta unit, 120 per notch (1/8 degree each).
    pub fn wheel(&mut self, x: f64, y: f64, delta_y: f64) {
        let mut steps = delta_y / 120.0;
        if self.prefs.invert_wheel {
            steps = -steps;
        }
        let focus = match self.zoom_focus_without_pick(x, y) {
            Some(focus) => focus,
            None => self.world_under_cursor(x, y),
        };
        match focus {
            Some(f) => {
                let diag = self.model_diag();
                self.camera.zoom_to(steps, f, 0.05f64.max(0.01 * diag));
            }
            None => self.camera.zoom(steps),
        }
        self.nav.zoom_focus = Some(ZoomFocus {
            pose: pose_key(&self.camera),
            at: ScreenPt::new(x, y),
            scene_version: self.scene.version,
            point: focus,
        });
        self.update();
    }

    /// The point a wheel notch at `x, y` zooms toward when the last notch
    /// already decided it: `Some(point)` (`point` may be `None`, a zoom
    /// toward nothing), or `None` when the notch has to pick it again.
    /// The same gesture (camera as the last notch left it, cursor within
    /// 24 px, model unchanged) reuses the last focus; otherwise a focus
    /// still within 24 px of the cursor on screen is kept.
    fn zoom_focus_without_pick(&self, x: f64, y: f64) -> Option<Option<DVec3>> {
        let cached = self.nav.zoom_focus?;
        let version = self.scene.version;
        if cached.pose == pose_key(&self.camera)
            && (x - cached.at.x).abs() < 24.0
            && (y - cached.at.y).abs() < 24.0
            && cached.scene_version == version
        {
            return Some(cached.point);
        }
        let f = cached.point.filter(|_| cached.scene_version == version)?;
        let p = self.world_to_pixel(f)?;
        ((p.x - x).abs() < 24.0 && (p.y - y).abs() < 24.0).then_some(Some(f))
    }

    /// Whether a wheel notch at `x, y` picks the point under the cursor.
    pub(super) fn wheel_picks(&self, x: f64, y: f64) -> bool {
        self.zoom_focus_without_pick(x, y).is_none()
    }

    // ---- keys ------------------------------------------------------------------------------
    /// Whether the Ctrl being released was pressed alone.
    pub fn ctrl_tapped(&self) -> bool {
        self.input.ctrl_alone
    }

    /// Shortcut override: does the viewport claim this key before the
    /// window's shortcuts see it? (A value being typed takes "2m"'s m.)
    pub fn shortcut_override(&mut self, ev: &KeyEvent) -> bool {
        if !is_modifier_key(ev.key) {
            self.input.ctrl_alone = false;
        }
        // `M3` starts with the letter of a shortcut.
        if self.value_box.buffer.is_empty()
            && !is_chord(ev.modifiers)
            && ev.text.eq_ignore_ascii_case("m")
            && self.value_spec().screw_sizes
        {
            return true;
        }
        if !self.value_box.buffer.is_empty() && !is_chord(ev.modifiers) {
            let t = ev.text.to_lowercase();
            if !t.is_empty()
                && (t.chars().all(|c| c.is_numeric())
                    || [
                        ".", ",", ";", " ", "-", ":", "\"", "'", "/", "m", "c", "r", "i", "n", "f",
                        "t", "x", "*",
                    ]
                    .contains(&t.as_str()))
            {
                return true;
            }
        }
        false
    }

    pub fn key_press(&mut self, ev: &KeyEvent) {
        self.input.key_pressed(ev);
        if ev.key == Key::Shift && !ev.auto_repeat {
            self.capture_shift_lock();
            let m = self.input.held_modifiers;
            self.refresh_snap_with(m);
        }
        if ev.key == Key::Alt && !ev.auto_repeat {
            self.apply_alt_cursor(true);
        } else if self.input.alt_tap && !ev.auto_repeat {
            self.input.alt_tap = false;
            self.input.alt_cycle_on_release = false;
        }
        // 1. The value box.
        if self.handle_value_key(ev) {
            return;
        }
        // 2. The tool.
        if self.active_tool().is_some() {
            let (k, m) = (ev.key, ev.modifiers);
            if self.with_tool(|t, vp| t.on_key(vp, k, m)).unwrap_or(false) {
                return;
            }
        }
        // 2b. Delete, with the tool idle.
        if ev.key == Key::Delete && !ev.auto_repeat {
            let busy = self.active_tool().is_some_and(|t| t.busy_for_delete());
            if !busy {
                crate::tools::select::delete_selection_or_hover(self);
                return;
            }
        }
        // 3. Esc.
        if ev.key == Key::Escape {
            self.escape();
            return;
        }
        // Projection toggle.
        if ev.key == Key::Char('P') && ev.modifiers.is_empty() {
            self.toggle_projection();
            return;
        }
        // Alt: cycle the linear inferences, only mid-operation. In a style
        // that navigates with Alt + move, an Alt held to move the camera
        // must not cycle: the cycle waits for the release of a clean tap.
        if ev.key == Key::Alt && !ev.auto_repeat {
            if self.tool_busy() {
                self.input.alt_tap = true;
                if self.alt_navigates() {
                    self.input.alt_cycle_on_release = true;
                } else {
                    self.cycle_linear_inference_mode();
                }
            }
            return;
        }
        // Axis lock.
        let lock = match ev.key {
            Key::Right => Some(Axis::X),
            Key::Left => Some(Axis::Y),
            Key::Up => Some(Axis::Z),
            _ => None,
        };
        if let Some(a) = lock {
            self.inference.axis_lock = if self.inference.axis_lock == Some(a) {
                None
            } else {
                Some(a)
            };
            self.refresh_snap();
            return;
        }
        if ev.key == Key::Down {
            self.cycle_reference_mode();
            self.refresh_snap();
        }
    }

    /// A key release; `true` when the viewport swallowed it (an Alt whose
    /// press it acted on: the menu bar must not take it).
    pub fn key_release(&mut self, ev: &KeyEvent) -> bool {
        self.input.key_released(ev);
        if self.active_tool().is_some() && !ev.auto_repeat {
            let k = ev.key;
            if let Some(Some(true)) = self.with_tool(|t, vp| t.on_key_release(vp, k)) {
                return true;
            }
        }
        if ev.key == Key::Alt && !ev.auto_repeat {
            self.apply_alt_cursor(false);
            if self.input.alt_tap {
                self.input.alt_tap = false;
                if std::mem::take(&mut self.input.alt_cycle_on_release) && self.tool_busy() {
                    self.cycle_linear_inference_mode();
                }
                return true;
            }
        }
        if ev.key == Key::Shift && !ev.auto_repeat {
            self.inference.shift_lock = None;
            let m = self.input.held_modifiers;
            self.refresh_snap_with(m);
        }
        false
    }

    /// Whether Alt + move moves the camera in the style (Touchpad).
    fn alt_navigates(&self) -> bool {
        self.prefs
            .nav_style
            .action(Trigger::Move, Modifiers::ALT)
            .is_some()
    }

    /// An Alt press acted on, its release still to swallow.
    pub fn alt_tap_pending(&self) -> bool {
        self.input.alt_tap
    }

    /// The keyboard went elsewhere: claims on pending releases lapse.
    pub fn focus_out(&mut self) {
        self.input.focus_out();
        self.nav.swallow_release = None;
    }

    fn apply_alt_cursor(&mut self, down: bool) {
        self.input.alt_down = down;
        if self.nav.mode.is_none() && self.active_tool().is_some_and(|t| t.info().key == "paint") {
            self.apply_tool_cursor();
        }
    }

    /// The application-wide Alt watcher: the shell calls it
    /// for EVERY key event, wherever the keyboard focus is, so the Paint
    /// pointer follows Alt and a clean tap toggles the eyedropper.
    pub fn app_key_event(&mut self, ev: &KeyEvent, press: bool) {
        if ev.auto_repeat {
            return;
        }
        if ev.key != Key::Alt {
            if press {
                self.input.alt_clean = false;
            }
            return;
        }
        let tap = !press && self.input.alt_clean;
        self.input.alt_clean = press;
        if press != self.input.alt_down {
            self.apply_alt_cursor(press);
        }
        if tap {
            self.alt_tapped();
        }
    }

    /// A mouse press anywhere: Alt+click is a sample, not a tap.
    pub fn app_mouse_press(&mut self) {
        self.input.alt_clean = false;
    }

    /// A clean Alt tap toggles Paint's eyedropper.
    pub fn alt_tapped(&mut self) -> bool {
        if self.nav.mode.is_some() || !self.active_tool().is_some_and(|t| t.info().key == "paint") {
            return false;
        }
        crate::tools::set_paint_sample_armed(!crate::tools::paint_sample_armed());
        self.apply_tool_cursor();
        true
    }

    /// Back to «all inferences» once the operation is over.
    pub fn release_linear_mode(&mut self) {
        if self.inference.linear_inference_mode == LinearMode::All {
            return;
        }
        if self.tool_busy() {
            return;
        }
        self.inference.linear_inference_mode = LinearMode::All;
        self.refresh_snap();
        self.update();
    }

    /// Drop the arrow-key axis lock when the operation that used it ends.
    pub fn release_axis_lock_after_operation(&mut self, committed: bool) {
        if self.inference.axis_lock.is_none() {
            return;
        }
        if !committed && self.tool_busy() {
            return;
        }
        self.inference.axis_lock = None;
        self.refresh_snap();
        self.update();
    }

    /// Alt: all → off → parallel/perp → all.
    pub fn cycle_linear_inference_mode(&mut self) {
        self.inference.linear_inference_mode = match self.inference.linear_inference_mode {
            LinearMode::All => LinearMode::Off,
            LinearMode::Off => LinearMode::ParallelPerp,
            LinearMode::ParallelPerp => LinearMode::All,
        };
        let label = match self.inference.linear_inference_mode {
            LinearMode::All => "Linear inferences: all on",
            LinearMode::Off => "Linear inferences: off",
            LinearMode::ParallelPerp => "Linear inferences: parallel / perpendicular only",
        };
        self.outbox
            .events
            .push(super::ViewportEvent::MeasurementChanged(tr(label, &[])));
        self.refresh_snap();
    }

    /// The Esc cascade, one step per press: the value box, a sticky
    /// constraint, the tool's operation, the selection, one group level,
    /// the tool's own cancel.
    pub fn escape(&mut self) {
        if !self.value_box.buffer.is_empty() {
            self.set_value_buffer("");
            return;
        }
        if self.release_constraints() {
            return;
        }
        if self.active_tool_key() == Some("paste") && self.has_window {
            self.outbox
                .events
                .push(super::ViewportEvent::ActivateTool("select".into()));
            return;
        }
        if self.tool_busy() {
            self.with_tool(|t, vp| t.on_cancel(vp));
            self.release_linear_mode();
            return;
        }
        if !self.scene.selection.is_empty() {
            self.scene.clear_selection();
            self.update();
            return;
        }
        if self.scene.edit_group().is_some() {
            self.end_one_group_edit();
            return;
        }
        self.with_tool(|t, vp| t.on_cancel(vp));
    }

    /// Drop the arrow-key axis lock and the Down-arrow reference.
    pub fn release_constraints(&mut self) -> bool {
        if self.inference.axis_lock.is_none() && self.inference.reference_mode.is_none() {
            return false;
        }
        self.inference.axis_lock = None;
        self.inference.clear_reference();
        self.refresh_snap();
        true
    }

    /// Down arrow: None → parallel → perpendicular → None.
    pub fn cycle_reference_mode(&mut self) {
        self.inference.cycle_reference_mode();
    }

    fn value_spec(&self) -> ValueSpec {
        self.active_tool()
            .map_or_else(ValueSpec::default, |t| t.value_spec())
    }

    /// The value box's share of a key press: typing, Backspace, Enter.
    pub fn handle_value_key(&mut self, ev: &KeyEvent) -> bool {
        if self.active_tool().is_none() {
            return false;
        }
        let spec = self.value_spec();
        if matches!(ev.key, Key::Return | Key::Enter) {
            if self.value_box.buffer.is_empty() {
                return false;
            }
            let mut vb = self.value_box.clone();
            let entry = vb.commit(spec);
            self.set_value_buffer("");
            match entry {
                None | Some(Entry::Nothing) => {}
                Some(Entry::Angle(deg)) => {
                    let v = ToolValue::Num(deg);
                    self.with_tool(|t, vp| t.on_value(vp, &v));
                }
                Some(Entry::Segments(n)) => {
                    if spec.segments {
                        let v = ToolValue::Segments(n);
                        self.with_tool(|t, vp| t.on_value(vp, &v));
                    }
                }
                Some(Entry::Array { count, kind, step }) => {
                    if let Some(array) = spec.array {
                        if step.is_none() || array.takes_step {
                            let v = ToolValue::Array { count, kind, step };
                            self.with_tool(|t, vp| t.on_value(vp, &v));
                        } else {
                            self.flash_status(&tr("Type the count alone here, e.g. 3x", &[]), 2500);
                        }
                    }
                }
                Some(Entry::Radius(r)) => {
                    if spec.radius {
                        let v = ToolValue::Radius(r);
                        self.with_tool(|t, vp| t.on_value(vp, &v));
                    }
                }
                Some(Entry::Screw { nominal, close }) => {
                    if spec.screw_sizes {
                        let v = ToolValue::Screw { nominal, close };
                        self.with_tool(|t, vp| t.on_value(vp, &v));
                    }
                }
                Some(Entry::Pattern(p)) => {
                    let v = ToolValue::Pattern(p);
                    self.with_tool(|t, vp| t.on_value(vp, &v));
                }
                Some(Entry::Value(v)) => {
                    if let Some(tv) = ToolValue::from_typed(&v) {
                        self.with_tool(|t, vp| t.on_value(vp, &tv));
                    }
                    self.release_axis_lock_after_operation(false);
                }
                Some(Entry::AbsoluteLength(v)) => {
                    if let Some(tv) = ToolValue::from_typed(&v) {
                        let tv = ToolValue::AbsLen(Box::new(tv));
                        self.with_tool(|t, vp| t.on_value(vp, &tv));
                    }
                    self.release_axis_lock_after_operation(false);
                }
            }
            return true;
        }
        let before = self.value_box.buffer.clone();
        let took = self.value_box.handle_key(ev, spec);
        if self.value_box.buffer != before {
            let b = self.value_box.buffer.clone();
            self.value_box.buffer = before;
            self.set_value_buffer(&b);
        }
        took
    }
}
