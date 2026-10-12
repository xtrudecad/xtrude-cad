// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What is actually drawing the viewport.
//!
//! A "terribly slow" report has one question behind it: is the viewport on
//! the GPU, or on a software rasteriser (llvmpipe, SwiftShader, WARP)? The
//! adapter the renderer got answers it; the About box shows it and
//! `xtrude-gl.txt` in the log folder keeps it.

use std::path::{Path, PathBuf};

/// Substrings of a renderer/vendor string that mean CPU rasterisation.
pub const SOFTWARE_MARKERS: [&str; 7] = [
    "gdi generic",
    "llvmpipe",
    "softpipe",
    "swiftshader",
    "microsoft basic render",
    "mesa offscreen",
    "software",
];

/// The strings that identify the drawing device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuInfo {
    pub vendor: String,
    pub renderer: String,
    /// Driver and graphics API version.
    pub version: String,
    pub software: bool,
    /// The windowing backend in use (wayland / x11).
    pub platform: String,
    /// The fallback that had to step in to get a device, if any.
    pub fallback: String,
}

impl GpuInfo {
    /// From the strings a device reports; `software` follows from them (or
    /// from `cpu_device`, an adapter that says it is the CPU).
    pub fn new(vendor: &str, renderer: &str, version: &str, cpu_device: bool) -> GpuInfo {
        let (vendor, renderer, version) = (vendor.trim(), renderer.trim(), version.trim());
        GpuInfo {
            vendor: vendor.to_string(),
            renderer: renderer.to_string(),
            version: version.to_string(),
            software: cpu_device || is_software_renderer(renderer, vendor),
            platform: String::new(),
            fallback: String::new(),
        }
    }

    /// From the adapter wgpu picked: its name is the renderer, its PCI
    /// vendor (or else its driver) the vendor, and the graphics API with
    /// the driver the version; a CPU adapter is software.
    pub fn from_adapter(a: &wgpu::AdapterInfo) -> GpuInfo {
        let vendor = match a.vendor {
            0x10de => "NVIDIA",
            0x1002 | 0x1022 => "AMD",
            0x8086 => "Intel",
            0x106b => "Apple",
            0x13b5 => "ARM",
            0x5143 => "Qualcomm",
            0x10005 => "Mesa",
            _ => a.driver.as_str(),
        };
        let api = match a.backend {
            wgpu::Backend::Vulkan => "Vulkan".to_string(),
            wgpu::Backend::Gl => "OpenGL".to_string(),
            wgpu::Backend::Metal => "Metal".to_string(),
            wgpu::Backend::Dx12 => "Direct3D 12".to_string(),
            other => format!("{other:?}"),
        };
        let version = [api.as_str(), a.driver.as_str(), a.driver_info.as_str()]
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        GpuInfo::new(
            vendor,
            &a.name,
            &version,
            a.device_type == wgpu::DeviceType::Cpu,
        )
    }
}

pub fn is_software_renderer(renderer: &str, vendor: &str) -> bool {
    let text = format!("{renderer} {vendor}").to_lowercase();
    SOFTWARE_MARKERS.iter().any(|m| text.contains(m))
}

/// One line for the About box: `renderer (vendor) — version`.
pub fn describe(info: &GpuInfo) -> String {
    if info.renderer.is_empty() {
        return String::new();
    }
    let mut parts = vec![info.renderer.clone()];
    if !info.vendor.is_empty() {
        parts.push(format!("({})", info.vendor));
    }
    if !info.version.is_empty() {
        parts.push(format!("— {}", info.version));
    }
    parts.join(" ")
}

/// Overwrite `xtrude-gl.txt` with the device strings; the path, or `None`
/// when nothing is writable.
pub fn write_gl_report(info: &GpuInfo, path: Option<&Path>) -> Option<PathBuf> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => xtrude_model::paths::user_log_dir()?.join("xtrude-gl.txt"),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let mut text = format!(
        "vendor   : {}\nrenderer : {}\nversion  : {}\nsoftware : {}\n",
        info.vendor,
        info.renderer,
        info.version,
        if info.software { "yes" } else { "no" }
    );
    if !info.platform.is_empty() {
        text += &format!("platform : {}\n", info.platform);
    }
    if !info.fallback.is_empty() {
        text += &format!("fallback : {}\n", info.fallback);
    }
    std::fs::write(&path, text).ok()?;
    Some(path)
}
