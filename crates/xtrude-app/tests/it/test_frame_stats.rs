// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Frame-time statistics behind `XTRUDE_FRAME_STATS`.

use std::time::{Duration, Instant};
use xtrude_app::shell::frame_stats::{
    backend_name, enabled_by, summarize, FrameSample, FrameStats, Summary,
};

#[test]
fn off_unless_the_switch_says_so() {
    assert!(!enabled_by(None));
    assert!(!enabled_by(Some("")));
    assert!(!enabled_by(Some("0")));
    assert!(enabled_by(Some("1")));
    assert!(enabled_by(Some("yes")));
}

#[test]
fn nearest_rank_percentiles() {
    let samples: Vec<f64> = (1..=10).rev().map(f64::from).collect();
    assert_eq!(
        summarize(&samples),
        Some(Summary {
            count: 10,
            p50: 5.0,
            p90: 9.0,
            max: 10.0,
        })
    );
    let one = summarize(&[7.0]).unwrap();
    assert_eq!((one.p50, one.p90, one.max), (7.0, 7.0, 7.0));
}

#[test]
fn no_samples_no_summary() {
    assert_eq!(summarize(&[]), None);
    assert_eq!(summarize(&[f64::NAN]), None);
}

fn sample(start: Instant, interactive: bool) -> FrameSample {
    FrameSample {
        start,
        interactive,
        cpu_seconds: Some(0.002),
        scale: 1.25,
    }
}

#[test]
fn only_intervals_between_interactive_frames_count() {
    let t0 = Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    let mut stats = FrameStats::default();
    stats.frame(sample(ms(0), true));
    stats.frame(sample(ms(16), true));
    // idle: the wait before the next drag stays out
    stats.frame(sample(ms(266), false));
    stats.frame(sample(ms(1000), true));
    stats.frame(sample(ms(1030), true));
    assert_eq!(stats.intervals.len(), 2);
    assert!((stats.intervals[0] - 16.0).abs() < 1e-9);
    assert!((stats.intervals[1] - 30.0).abs() < 1e-9);
    // CPU time of the four interactive frames, in ms
    assert_eq!(stats.cpu.len(), 4);
    assert!((stats.cpu[0] - 2.0).abs() < 1e-4);
    assert_eq!(stats.scale, 1.25);
}

#[test]
fn the_report_names_backend_scale_and_percentiles() {
    let t0 = Instant::now();
    let mut stats = FrameStats::default();
    stats.backend = Some("Wayland");
    stats.frame(sample(t0, true));
    stats.frame(sample(t0 + Duration::from_millis(20), true));
    let report = stats.report();
    assert!(report.contains("backend Wayland, scale 1.25"), "{report}");
    assert!(
        report.contains("frame interval: 1 frames, p50 20.0 ms, p90 20.0 ms, max 20.0 ms"),
        "{report}"
    );
    assert!(
        report.contains("CPU per frame : 2 frames, p50 2.0 ms"),
        "{report}"
    );
    let empty = FrameStats::default().report();
    assert!(empty.contains("backend unknown"), "{empty}");
    assert!(
        empty.contains("frame interval: no interactive frames"),
        "{empty}"
    );
}

#[test]
fn backend_from_the_display_handle() {
    use std::ptr::NonNull;
    use wgpu::rwh::{RawDisplayHandle, WaylandDisplayHandle, XcbDisplayHandle};
    let mut dummy = 0u8;
    let wl = WaylandDisplayHandle::new(NonNull::from(&mut dummy).cast());
    assert_eq!(backend_name(&RawDisplayHandle::Wayland(wl)), "Wayland");
    let xcb = XcbDisplayHandle::new(None, 0);
    assert_eq!(backend_name(&RawDisplayHandle::Xcb(xcb)), "X11");
}
