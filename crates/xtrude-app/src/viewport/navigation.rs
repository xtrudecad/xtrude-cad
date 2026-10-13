// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The camera navigation state and the rubber-band boxes.

use glam::DMat4;

use super::depth::DepthSnap;
use super::events::ZoomFocus;
use super::nav_style::NavAction;
use super::NavMode;
use crate::input::{Button, MouseEvent, ScreenPt};

/// What holds a camera drag on: it ends when that lets go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragBy {
    Button(Button),
    /// The left and the right button together: either release ends it.
    Chord,
    /// A move with modifiers and no button: it ends when they change.
    Move,
}

/// The pointer state of a camera drag and its gesture.
#[derive(Clone, Copy, Debug)]
pub(super) struct CameraDrag {
    pub(super) last: (i64, i64),
    pub(super) by: DragBy,
    pub(super) action: NavAction,
}

/// A right press that may still be a click (the context menu): where and
/// when it went down.
#[derive(Clone, Copy, Debug)]
pub(super) struct RightPress {
    pub(super) at: ScreenPt,
    pub(super) time_ms: u64,
}

/// A rectangle dragged on screen (box select, Zoom Window): it exists
/// only while its drag is under way, from the press to the release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RubberBand {
    pub(super) start: ScreenPt,
    pub(super) cur: ScreenPt,
}

impl RubberBand {
    /// A band pressed at `at`, still empty.
    pub(super) fn at(at: ScreenPt) -> RubberBand {
        RubberBand { start: at, cur: at }
    }

    /// `(start, current)` as pixel tuples.
    pub(super) fn corners(&self) -> ((f64, f64), (f64, f64)) {
        (self.start.tuple(), self.cur.tuple())
    }
}

/// Camera navigation: the navigation mode a left drag takes, the camera
/// drag under way and its gesture, the look drag, the wheel's zoom
/// target and the depth of the frame on screen.
///
/// `pan_depth` is the depth a pan of `camera_drag` slides at; `zoom_box`
/// is the rectangle of a Zoom Window drag, from its press to its
/// release. `right_press` is
/// set from a right press until its release, unless it moved too far or
/// joined a chord; `swallow_release` is the button a chord left held,
/// whose release does nothing, until an event shows that release lost
/// (`release_lost_unless_held`), the focus goes or the drag state is
/// reset (`end_drag`).
#[derive(Debug, Default)]
pub struct Navigation {
    /// The camera navigation mode for a left drag (trackpads).
    pub mode: Option<NavMode>,
    pub(super) camera_drag: Option<CameraDrag>,
    pub(super) pan_depth: Option<f64>,
    pub(super) zoom_box: Option<RubberBand>,
    pub(super) zoom_focus: Option<ZoomFocus>,
    pub(super) right_press: Option<RightPress>,
    pub(super) swallow_release: Option<Button>,
    /// The camera and size of the last frame drawn for the screen.
    pub(super) painted_view: Option<(DMat4, [u32; 2])>,
    /// That frame's depth, once read back
    /// ([`super::Viewport::set_depth_snap`]).
    pub(super) depth_snap: Option<DepthSnap>,
}

impl Navigation {
    /// The camera drag's gesture is over.
    pub(super) fn end_drag(&mut self) {
        self.camera_drag = None;
        self.swallow_release = None;
    }

    /// The release a chord left to swallow was lost if `ev` (a press or
    /// a move) shows its button up, or pressing again: it no longer waits.
    pub(super) fn release_lost_unless_held(&mut self, ev: &MouseEvent) {
        let Some(b) = self.swallow_release else {
            return;
        };
        let held = match b {
            Button::Left => ev.left,
            Button::Middle => ev.middle,
            Button::Right => ev.right,
        };
        if !held || ev.button == Some(b) {
            self.swallow_release = None;
        }
    }
}
