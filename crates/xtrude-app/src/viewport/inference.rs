// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The snapping memory: what the inference engine keeps between hovers.

use std::time::Instant;

use glam::DVec3;
use xtrude_core::snap::{LinearMode, RefMode, Rgb, SnapResult};
use xtrude_core::Axis;

use super::{CenterRef, PickedEdge};
use crate::tools::Plane;

/// The inference state carried from one hover to the next: the locks
/// (arrow-key axis, Shift, Down-arrow reference, the Alt linear mode),
/// the hovered edge and centre, the points and directions acquired along
/// the way, the encouraged points and the dwell that makes them.
///
/// The Down-arrow cycle sets `reference_edge` and `reference_mode`
/// together, and they are cleared together; a dwell is timed
/// (`dwell_since`) only on a `dwell_point`; at most two points are
/// `encouraged`, the newest last.
#[derive(Debug)]
pub struct Inference {
    /// The arrow-key axis lock.
    pub axis_lock: Option<Axis>,
    pub last_snap: Option<SnapResult>,
    /// The Down-arrow reference: the edge and how the snap follows it.
    pub reference_edge: Option<PickedEdge>,
    pub reference_mode: Option<RefMode>,
    /// The Alt cycle of the linear inferences.
    pub linear_inference_mode: LinearMode,
    /// The Shift lock: a direction and its colour.
    pub shift_lock: Option<(DVec3, Rgb)>,
    pub hover_edge: Option<PickedEdge>,
    pub(super) center_ref: Option<CenterRef>,
    pub(super) hover_center: Option<CenterRef>,
    pub acquired_edge: Option<(DVec3, DVec3)>,
    pub acquired_point: Option<DVec3>,
    pub(crate) acquired_face_normal: Option<DVec3>,
    /// The last two points the cursor paused on.
    pub encouraged: Vec<DVec3>,
    pub(super) dwell_point: Option<DVec3>,
    pub(super) dwell_since: Option<Instant>,
    /// The plane the last pixel→world used.
    pub(crate) last_work_plane: Option<Plane>,
}

impl Default for Inference {
    fn default() -> Self {
        Inference {
            axis_lock: None,
            last_snap: None,
            reference_edge: None,
            reference_mode: None,
            linear_inference_mode: LinearMode::All,
            shift_lock: None,
            hover_edge: None,
            center_ref: None,
            hover_center: None,
            acquired_edge: None,
            acquired_point: None,
            acquired_face_normal: None,
            encouraged: Vec::new(),
            dwell_point: None,
            dwell_since: None,
            last_work_plane: None,
        }
    }
}

impl Inference {
    /// Another tool takes over: its operation starts with no lock, no
    /// snap and nothing acquired.
    pub(super) fn tool_changed(&mut self) {
        self.linear_inference_mode = LinearMode::All;
        self.axis_lock = None;
        self.last_snap = None;
        self.center_ref = None;
        self.acquired_edge = None;
        self.acquired_point = None;
        self.acquired_face_normal = None;
        self.encouraged.clear();
        self.dwell_point = None;
        self.dwell_since = None;
    }

    /// Drop the Down-arrow reference.
    pub(super) fn clear_reference(&mut self) {
        self.reference_edge = None;
        self.reference_mode = None;
    }

    /// Down arrow: none → parallel to the hovered edge → perpendicular →
    /// none. Without a hovered edge there is nothing to start from.
    pub(super) fn cycle_reference_mode(&mut self) {
        match self.reference_mode {
            None => {
                let Some(e) = self.hover_edge.clone() else {
                    return;
                };
                self.reference_edge = Some(e);
                self.reference_mode = Some(RefMode::Parallel);
            }
            Some(RefMode::Parallel) => self.reference_mode = Some(RefMode::Perpendicular),
            Some(_) => self.clear_reference(),
        }
    }

    /// The cursor is over `point` (or nothing): (re)start the pause that
    /// turns it into an encouraged point.
    pub(super) fn dwell_on(&mut self, point: Option<DVec3>) {
        match point {
            None => {
                self.dwell_point = None;
                self.dwell_since = None;
            }
            Some(p) => {
                if self.dwell_point.is_some_and(|d| (d - p).length() < 1e-6) {
                    return;
                }
                self.dwell_point = Some(p);
                self.dwell_since = Some(Instant::now());
            }
        }
    }

    /// Make `point` the newest encouraged point (two are kept) and the
    /// 'from point' reference.
    pub(super) fn encourage(&mut self, point: DVec3) {
        self.encouraged.retain(|p| (*p - point).length() > 1e-6);
        self.encouraged.push(point);
        let n = self.encouraged.len();
        if n > 2 {
            self.encouraged.drain(..n - 2);
        }
        self.acquired_point = Some(point);
    }
}
