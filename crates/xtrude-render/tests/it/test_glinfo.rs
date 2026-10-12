// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Who draws the viewport (the strings come from the wgpu adapter).

use xtrude_render::gpuinfo::{describe, is_software_renderer, write_gl_report, GpuInfo};

#[test]
fn test_software_rasterisers_are_recognised_and_gpus_are_not() {
    assert!(is_software_renderer("GDI Generic", "Microsoft Corporation"));
    assert!(is_software_renderer(
        "llvmpipe (LLVM 17.0.6, 256 bits)",
        "Mesa"
    ));
    assert!(is_software_renderer("Google SwiftShader", ""));
    assert!(!is_software_renderer(
        "NVIDIA GeForce RTX 3060/PCIe/SSE2",
        "NVIDIA Corporation"
    ));
    assert!(!is_software_renderer(
        "Intel(R) Iris(R) Xe Graphics",
        "Intel"
    ));
    assert!(!is_software_renderer(
        "AMD Radeon RX 6600 (radeonsi, navi23)",
        "AMD"
    ));
}

#[test]
fn test_read_gl_info_decodes_bytes_and_flags_software() {
    let info = GpuInfo::new("Microsoft Corporation", "GDI Generic", "1.1.0", false);
    assert_eq!(
        (
            info.vendor.as_str(),
            info.renderer.as_str(),
            info.version.as_str(),
            info.software
        ),
        ("Microsoft Corporation", "GDI Generic", "1.1.0", true)
    );
    let gpu = GpuInfo::new(
        "NVIDIA Corporation",
        "NVIDIA GeForce GTX 1650/PCIe/SSE2",
        "OpenGL 3.3.0 NVIDIA 552.44",
        false,
    );
    assert!(!gpu.software);
    assert_eq!(
        describe(&gpu),
        "NVIDIA GeForce GTX 1650/PCIe/SSE2 (NVIDIA Corporation) — OpenGL 3.3.0 NVIDIA 552.44"
    );
    // an adapter that says it is the CPU is software whatever its name
    assert!(GpuInfo::new("Mesa", "Some name", "", true).software);
}

#[test]
fn test_a_silent_driver_never_breaks_start_up() {
    let info = GpuInfo::new("", "", "", false);
    assert_eq!(info, GpuInfo::default());
    assert_eq!(describe(&info), "");
}

#[test]
fn test_the_report_lands_in_the_log_folder() {
    let dir = std::env::temp_dir().join(format!("xtrude-gl-{}", std::process::id()));
    let info = GpuInfo::new(
        "Intel",
        "Intel(R) UHD Graphics 620",
        "4.6.0 - Build 31.0.101.2111",
        false,
    );
    let out = write_gl_report(&info, Some(&dir.join("logs").join("xtrude-gl.txt"))).unwrap();
    let text = std::fs::read_to_string(out).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.contains("renderer : Intel(R) UHD Graphics 620"));
    assert!(text.contains("software : no"));
}

fn adapter(name: &str, vendor: u32, driver: &str, cpu: bool) -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: name.into(),
        vendor,
        device: 0,
        device_type: if cpu {
            wgpu::DeviceType::Cpu
        } else {
            wgpu::DeviceType::DiscreteGpu
        },
        device_pci_bus_id: String::new(),
        driver: driver.into(),
        driver_info: "Mesa 25.1".into(),
        backend: wgpu::Backend::Vulkan,
        subgroup_min_size: 0,
        subgroup_max_size: 0,
        transient_saves_memory: None,
        limit_bucket: None,
    }
}

#[test]
fn test_the_wgpu_adapter_becomes_the_device_strings() {
    let gpu = GpuInfo::from_adapter(&adapter(
        "AMD Radeon RX 6600 (RADV NAVI23)",
        0x1002,
        "radv",
        false,
    ));
    assert_eq!(gpu.vendor, "AMD");
    assert_eq!(gpu.renderer, "AMD Radeon RX 6600 (RADV NAVI23)");
    assert_eq!(gpu.version, "Vulkan radv Mesa 25.1");
    assert!(!gpu.software);
    // llvmpipe: a CPU adapter, Mesa's vendor id
    let cpu = GpuInfo::from_adapter(&adapter(
        "llvmpipe (LLVM 19.1.7, 256 bits)",
        0x10005,
        "llvmpipe",
        true,
    ));
    assert_eq!(cpu.vendor, "Mesa");
    assert!(cpu.software);
    // an unknown vendor id falls back to the driver's name
    let mut gl = adapter("Some GPU", 0x1234, "somedrv", false);
    gl.backend = wgpu::Backend::Gl;
    let other = GpuInfo::from_adapter(&gl);
    assert_eq!(other.vendor, "somedrv");
    assert_eq!(other.version, "OpenGL somedrv Mesa 25.1");
}
