// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `xtrude-convert IN OUT`: open a document and write it back out in the
//! format of OUT's extension.
//!
//! IN may also be a STEP file (`.step`, `.stp`): it is read by the
//! `xtrude-step` helper (next to this program, or named by
//! `XTRUDE_STEP_HELPER`) with the default tessellation, and what the
//! import left out is printed on stderr.

use std::path::Path;
use std::process::ExitCode;

use xtrude_model::scene::Scene;

fn run(input: &Path, output: &Path) -> Result<(), String> {
    let mut scene = Scene::new();
    let ext = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    match ext(input).as_str() {
        "igz" => xtrude_io::igz::load_into(&mut scene, input, None).map_err(|e| e.to_string())?,
        // STL files are millimetres (what the exporters write)
        "stl" => xtrude_io::stl::load_stl(
            &mut scene,
            input,
            None,
            0.001,
            xtrude_io::stl::SimplifyMode::None,
        )
        .map_err(|e| e.to_string())?,
        "obj" => xtrude_io::obj::load_obj(&mut scene, input, None, 1.0, None)
            .map_err(|e| e.to_string())?,
        "step" | "stp" => {
            let warnings = xtrude_io::step::load_step(
                &mut scene,
                input,
                &xtrude_io::step::Tessellation::default(),
                None,
            )?;
            for w in warnings {
                eprintln!("xtrude-convert: {w}");
            }
        }
        other => return Err(format!("cannot read .{other}")),
    }
    match ext(output).as_str() {
        "igz" => xtrude_io::igz::save_scene(&scene, output).map_err(|e| e.to_string())?,
        "stl" => xtrude_io::stl::save_stl(&scene, output, "mm").map_err(|e| e.to_string())?,
        "obj" => xtrude_io::obj::save_obj(&scene, output).map_err(|e| e.to_string())?,
        other => return Err(format!("cannot write .{other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: xtrude-convert IN OUT");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtrude-convert: {e}");
            ExitCode::FAILURE
        }
    }
}
