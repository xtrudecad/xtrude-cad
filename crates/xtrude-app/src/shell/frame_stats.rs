// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Frame-time statistics, for measuring the windowing backends against
//! each other (Wayland vs X11 at a fractional scale; see
//! [`crate::platform_choice`]).
//!
//! Off by default. With `XTRUDE_FRAME_STATS=1` in the environment the
//! shell records every frame drawn while the user interacts (a frame with
//! input events: pointer motion, buttons, keys) and, on exit, prints to
//! stderr the backend in use, the scale factor, and p50 / p90 / max of:
//!
//! - the **frame interval**, from the start of one frame to the start of
//!   the next: what the user sees, including the compositor's share;
//! - the **CPU time** eframe spent on a frame (building the UI,
//!   tessellating, submitting the paint).
//!
//! Only intervals between two consecutive interactive frames count, so the
//! idle waits (before a drag, or while the mouse rests with a button held,
//! when nothing draws or a timer's frame does) stay out of the numbers.

use std::time::Instant;

/// The environment variable that turns the statistics on.
pub const ENV_SWITCH: &str = "XTRUDE_FRAME_STATS";

/// Whether the switch's value turns the statistics on: any value but
/// empty or `0`.
pub fn enabled_by(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.trim().is_empty() && v.trim() != "0")
}

/// p50 / p90 / max of a set of samples, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub p50: f64,
    pub p90: f64,
    pub max: f64,
}

/// The nearest-rank percentiles of `samples`; `None` when there are none.
pub fn summarize(samples: &[f64]) -> Option<Summary> {
    let mut sorted: Vec<f64> = samples.iter().copied().filter(|s| s.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let rank = |p: f64| {
        let i = (p * sorted.len() as f64).ceil() as usize;
        sorted[i.clamp(1, sorted.len()) - 1]
    };
    Some(Summary {
        count: sorted.len(),
        p50: rank(0.5),
        p90: rank(0.9),
        max: sorted[sorted.len() - 1],
    })
}

/// One frame as the shell sees it.
#[derive(Clone, Copy, Debug)]
pub struct FrameSample {
    /// When the frame started.
    pub start: Instant,
    /// Whether the user was interacting during the frame.
    pub interactive: bool,
    /// The CPU time eframe reports for the previous frame, in seconds.
    pub cpu_seconds: Option<f32>,
    /// The window's native pixels per point.
    pub scale: f32,
}

/// The frames recorded so far.
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    /// Frame intervals between interactive frames, in ms.
    pub intervals: Vec<f64>,
    /// CPU time of the interactive frames, in ms.
    pub cpu: Vec<f64>,
    /// The backend in use (`Wayland`, `X11`, …), once known.
    pub backend: Option<&'static str>,
    /// The scale factor of the last frame.
    pub scale: f32,
    /// The start of the previous frame, if it was interactive.
    last_interactive: Option<Instant>,
}

impl FrameStats {
    /// Record one frame.
    pub fn frame(&mut self, sample: FrameSample) {
        self.scale = sample.scale;
        if !sample.interactive {
            self.last_interactive = None;
            return;
        }
        if let Some(prev) = self.last_interactive {
            let ms = sample.start.saturating_duration_since(prev).as_secs_f64() * 1000.0;
            self.intervals.push(ms);
        }
        if let Some(cpu) = sample.cpu_seconds {
            self.cpu.push(f64::from(cpu) * 1000.0);
        }
        self.last_interactive = Some(sample.start);
    }

    /// The text printed on exit.
    pub fn report(&self) -> String {
        let line = |name: &str, samples: &[f64]| match summarize(samples) {
            Some(s) => format!(
                "  {name}: {} frames, p50 {:.1} ms, p90 {:.1} ms, max {:.1} ms",
                s.count, s.p50, s.p90, s.max
            ),
            None => format!("  {name}: no interactive frames"),
        };
        format!(
            "Xtrude CAD frame stats: backend {}, scale {:.2}\n{}\n{}",
            self.backend.unwrap_or("unknown"),
            self.scale,
            line("frame interval", &self.intervals),
            line("CPU per frame ", &self.cpu),
        )
    }
}

/// The backend a display handle belongs to.
pub fn backend_name(display: &wgpu::rwh::RawDisplayHandle) -> &'static str {
    use wgpu::rwh::RawDisplayHandle;
    match display {
        RawDisplayHandle::Wayland(_) => "Wayland",
        RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => "X11",
        RawDisplayHandle::Windows(_) => "Windows",
        RawDisplayHandle::AppKit(_) => "macOS",
        _ => "other",
    }
}
