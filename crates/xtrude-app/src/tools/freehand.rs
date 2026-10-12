// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Freehand tool: press and drag to sketch; the stroke is simplified in screen
//! space and lands as one curve; a stroke ending at its start closes into a
//! face.

use glam::DVec3;

use super::arc::commit_arc;
use super::{Plane, SnapPrefs, Tool, ToolContext, ToolInfo, WorkPlane};
use crate::tool_any;
use crate::viewport::Viewport;

const MIN_SAMPLE_PX: f64 = 3.0;
const SIMPLIFY_PX: f64 = 1.6;
const CLOSE_PX: f64 = 10.0;

/// Mark the screen-space RDP survivors between `i0` and `i1`.
fn rdp(i0: usize, i1: usize, s: &[(f64, f64)], keep: &mut [bool]) {
    let (ax, ay) = s[i0];
    let (bx, by) = s[i1];
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let mut worst: Option<usize> = None;
    let mut worst_d = SIMPLIFY_PX;
    for (i, &(px, py)) in s.iter().enumerate().take(i1).skip(i0 + 1) {
        let d = if l2 < 1e-12 {
            (px - ax).hypot(py - ay)
        } else {
            let t = (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0);
            (px - (ax + t * dx)).hypot(py - (ay + t * dy))
        };
        if d > worst_d {
            worst = Some(i);
            worst_d = d;
        }
    }
    if let Some(w) = worst {
        keep[w] = true;
        rdp(i0, w, s, keep);
        rdp(w, i1, s, keep);
    }
}

#[derive(Default)]
pub struct FreehandTool {
    pub work_plane: Option<Plane>,
    pub start_point: Option<DVec3>,
    world: Vec<DVec3>,
    screen: Vec<(f64, f64)>,
    stroke: bool,
}

impl FreehandTool {
    pub fn new() -> FreehandTool {
        FreehandTool::default()
    }

    fn reset(&mut self) {
        *self = FreehandTool::new();
    }
}

impl Tool for FreehandTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("freehand"),
            description: Some(
                "Drag to sketch a curve; a stroke that ends where it began closes into a face.",
            ),
            ..ToolInfo::new("freehand", "Freehand")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_click(&mut self, _vp: &mut Viewport, ctx: &ToolContext) {
        self.stroke = true;
        self.start_point = Some(ctx.world);
        self.world = vec![ctx.world];
        self.screen = vec![ctx.screen];
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        if !self.stroke {
            return;
        }
        let (sx, sy) = ctx.screen;
        let Some(&(lx, ly)) = self.screen.last() else {
            return;
        };
        if (sx - lx).powi(2) + (sy - ly).powi(2) < MIN_SAMPLE_PX * MIN_SAMPLE_PX {
            return;
        }
        self.world.push(ctx.world);
        self.screen.push((sx, sy));
        vp.update();
    }

    fn on_release(&mut self, vp: &mut Viewport) {
        if !self.stroke {
            return;
        }
        let world = std::mem::take(&mut self.world);
        let screen = std::mem::take(&mut self.screen);
        self.reset();
        if world.len() < 3 {
            vp.update();
            return;
        }
        let (sx, sy) = screen[0];
        let (ex, ey) = screen[screen.len() - 1];
        let closed = (ex - sx).powi(2) + (ey - sy).powi(2) <= CLOSE_PX * CLOSE_PX;
        let mut keep = vec![false; world.len()];
        keep[0] = true;
        let last = world.len() - 1;
        keep[last] = true;
        rdp(0, last, &screen, &mut keep);
        let mut pts: Vec<DVec3> = world
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(p, _)| *p)
            .collect();
        if closed && pts.len() >= 3 {
            let n = pts.len();
            pts[n - 1] = pts[0];
        }
        if pts.len() >= 2 {
            commit_arc(vp, &pts, None, None);
        }
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        self.world.windows(2).map(|w| (w[0], w[1])).collect()
    }

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
    fn busy(&self) -> bool {
        self.start_point.is_some() || self.stroke
    }
}
