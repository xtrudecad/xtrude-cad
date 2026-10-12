// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The device drawing the viewport reaches the About box and the log
//! folder's xtrude-gl.txt, with a word when it is a software rasteriser.

use crate::common;

use common::TmpDir;
use xtrude_render::gpuinfo::GpuInfo;

#[test]
fn test_the_about_box_names_the_device_and_the_report_is_written() {
    let tmp = TmpDir::new();
    let mut w = common::window();
    assert_eq!(w.gpu_line(), None, "nothing before the shell has a device");
    let report = tmp.join("logs").join("xtrude-gl.txt");
    let info = GpuInfo::new("AMD", "AMD Radeon RX 6600", "Vulkan radv", false);
    assert_eq!(w.set_gpu_info(info, Some(&report)), Some(report.clone()));
    let line = w.gpu_line().expect("a line");
    assert!(
        line.contains("AMD Radeon RX 6600 (AMD) — Vulkan radv"),
        "{line}"
    );
    assert!(!line.contains("software"), "{line}");
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains("renderer : AMD Radeon RX 6600"), "{text}");
}

#[test]
fn test_a_software_rasteriser_is_called_slow() {
    let tmp = TmpDir::new();
    let mut w = common::window();
    let info = GpuInfo::new("Mesa", "llvmpipe (LLVM 19.1.7, 256 bits)", "Vulkan", true);
    w.set_gpu_info(info, Some(&tmp.join("xtrude-gl.txt")));
    assert!(w.gpu_line().unwrap().contains("software rendering, slow"));
}

#[test]
fn test_a_silent_driver_leaves_the_about_box_alone() {
    let tmp = TmpDir::new();
    let mut w = common::window();
    w.set_gpu_info(GpuInfo::default(), Some(&tmp.join("xtrude-gl.txt")));
    assert_eq!(w.gpu_line(), None);
}
