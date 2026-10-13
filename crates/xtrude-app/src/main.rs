// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Xtrude CAD — technical 3D modelling for 3D printing, in Rust.
//!
//! `xtrude [document.xtr]` (an IngeTrazo `document.igz` opens as a new,
//! unsaved document).

use std::path::PathBuf;
use xtrude_model::settings::{self, keys};

use xtrude_app::gpu_fallback::{self, Outcome};
use xtrude_app::platform_choice::{self, ENV_OVERRIDE, WAYLAND, XCB};
use xtrude_app::shell::XtrudeApp;
use xtrude_app::window::MainWindow;
use xtrude_app::APP_ID;
use xtrude_render::gpuinfo::GpuInfo;

fn icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../resources/xtrude_256.png");
    eframe::icon_data::from_png_bytes(bytes).ok()
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    if std::env::args().any(|a| a == "--version") {
        println!("Xtrude CAD {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if std::env::args().any(|a| a == "--check") {
        std::process::exit(self_check());
    }
    xtrude_model::crash::install_panic_hook();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Xtrude CAD")
        .with_app_id(APP_ID)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 400.0]);
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    // Which windowing backend: an explicit
    // XTRUDE_PLATFORM wins, then Preferences ▸ Display ▸ Graphics server.
    let forced = std::env::var(ENV_OVERRIDE).ok().filter(|v| !v.is_empty());
    let preference = settings::read(&keys::PLATFORM);
    let backend: Option<String> =
        forced.or_else(|| platform_choice::platform_for(&preference).map(str::to_string));
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let on_wayland = platform_choice::on_wayland(backend.as_deref(), &env);
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        event_loop_builder: event_loop_hook(backend.as_deref()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Xtrude CAD",
        options,
        Box::new(move |cc| {
            let mut window = MainWindow::new();
            if let Some(rs) = cc.wgpu_render_state.as_ref() {
                let mut info = GpuInfo::from_adapter(&rs.adapter.get_info());
                info.platform = (if on_wayland { "wayland" } else { "x11" }).to_string();
                if gpu_fallback::Env::current().guard {
                    info.fallback = "restarted under X11".to_string();
                }
                window.set_gpu_info(info, None);
            }
            window.load_extensions(xtrude_ext::default_dir());
            if let Some(doc) = args.first().map(PathBuf::from) {
                if doc.exists() {
                    window.open_on_start(&doc);
                }
            }
            Ok(Box::new(XtrudeApp::new(cc, window)))
        }),
    );
    // A driver that gives no context under Wayland gets one more try under
    // X11; the guard stops a second restart.
    let env = gpu_fallback::Env::current();
    let outcome = match &result {
        Ok(()) => gpu_fallback::on_success(&env),
        Err(_) => gpu_fallback::on_failure(if on_wayland { "wayland" } else { "x11" }, &env),
    };
    if let Some(msg) = gpu_fallback::message(outcome) {
        eprintln!("{msg}");
    }
    if outcome == Outcome::Restart {
        restart_under_x11();
    }
    result
}

/// Report whether this machine has what the app needs at run time
/// (`--check`). The shaders and the icon are built
/// into the binary; what can be missing is a GPU adapter and the system
/// fonts 3D Text draws with.
fn self_check() -> i32 {
    println!("Xtrude CAD {}", env!("CARGO_PKG_VERSION"));
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!("  executable     : {exe}");
    println!("  shaders, icon  : built in");
    let mut problems = Vec::new();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(a) => {
            let info = a.get_info();
            let driver = if info.driver.is_empty() {
                String::new()
            } else {
                format!(", {}", info.driver)
            };
            println!(
                "  GPU adapter    : found  {} ({:?}{driver})",
                info.name, info.backend
            );
        }
        Err(e) => {
            println!("  GPU adapter    : MISSING  ({e})");
            problems.push("GPU adapter");
        }
    }
    let fonts = xtrude_core::text3d::vertical_metrics("", false, false).is_some();
    println!(
        "  fonts (3D Text): {}",
        if fonts { "found" } else { "MISSING" }
    );
    if !fonts {
        problems.push("fonts");
    }
    if problems.is_empty() {
        println!("\nOK");
        0
    } else {
        println!("\nNOT OK — missing: {}", problems.join(", "));
        1
    }
}

/// The event-loop hook forcing a backend (`xcb` / `wayland`), if any.
fn event_loop_hook(backend: Option<&str>) -> Option<eframe::EventLoopBuilderHook> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;
        match backend {
            Some(XCB) => {
                return Some(Box::new(|b| {
                    b.with_x11();
                }))
            }
            Some(WAYLAND) => {
                return Some(Box::new(|b| {
                    b.with_wayland();
                }))
            }
            _ => {}
        }
    }
    let _ = backend;
    None
}

/// Re-exec this very process under X11, guarded against a second restart.
fn restart_under_x11() {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let err = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(gpu_fallback::GUARD_ENV, "1")
            .env(ENV_OVERRIDE, XCB)
            .exec();
        eprintln!("Xtrude CAD: could not restart under X11: {err}");
    }
}
