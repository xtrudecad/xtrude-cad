// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The example extensions shipped in `resources/extensions/examples` load,
//! and each command builds what it says with its default parameters.

use std::path::PathBuf;

use xtrude_ext::{discover, Extension};
use xtrude_model::solids::solid_volume;
use xtrude_model::{History, Scene};

use crate::common::TmpDir;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/extensions/examples")
}

fn examples() -> Vec<Extension> {
    discover(&examples_dir())
        .into_iter()
        .map(|r| r.unwrap_or_else(|(path, e)| panic!("{}: {}", path.display(), e.report())))
        .collect()
}

/// Runs `ext`'s command `cmd` with its defaults in a new mm document.
fn run(ext: &Extension, cmd: &str, log: &TmpDir) -> Scene {
    xtrude_model::settings::use_memory_store();
    let mut scene = Scene::new();
    let mut history = History::new();
    history.error_log = Some(log.0.join("errors.log"));
    let defaults = ext.command(cmd).expect("the command").defaults();
    let mut command = ext
        .script_command(cmd, &defaults)
        .expect("the command")
        .expect("its defaults");
    assert!(
        history.execute(&mut scene, &mut command),
        "{}",
        history.last_error.unwrap_or_default()
    );
    assert_eq!(history.undo_len(), 1);
    scene
}

/// Each group's name and volume (mm³), if it is a solid.
fn solids(scene: &Scene) -> Vec<(String, Option<f64>)> {
    scene
        .doc
        .groups
        .iter()
        .map(|g| {
            let vol = solid_volume(&scene.doc.meshes, g).map(|v| v * 1e9);
            (g.name.clone(), vol)
        })
        .collect()
}

#[test]
fn every_example_loads() {
    let ids: Vec<String> = examples().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, ["box", "gears"]);
}

#[test]
fn the_box_example_builds_a_box_and_its_lid() {
    let log = TmpDir::new();
    let exts = examples();
    let scene = run(&exts[0], "box", &log);
    let got = solids(&scene);
    assert_eq!(got.len(), 2, "{got:?}");
    // 60 × 40 × 25 with 2 mm walls, hollow above a 2 mm floor
    let box_volume = 60.0 * 40.0 * 25.0 - 56.0 * 36.0 * 23.0;
    // a 2 mm plate and a 3 mm lip 2.2 mm inside its edges
    let lid_volume = 60.0 * 40.0 * 2.0 + 55.6 * 35.6 * 3.0;
    assert_eq!(got[0].0, "Box");
    assert_eq!(got[1].0, "Lid");
    let close = |v: Option<f64>, want: f64| v.is_some_and(|v| (v - want).abs() < 1e-3);
    assert!(close(got[0].1, box_volume), "{got:?}");
    assert!(close(got[1].1, lid_volume), "{got:?}");
    assert_eq!(scene.selection.len(), 2);
    let (lo, hi) = scene.compute_bounds().expect("geometry");
    assert!(
        lo.z.abs() < 1e-9 && (hi.z - 0.025).abs() < 1e-9,
        "{lo} {hi}"
    );
}

#[test]
fn the_gear_example_builds_a_solid_gear() {
    let log = TmpDir::new();
    let exts = examples();
    let scene = run(&exts[1], "spur", &log);
    let got = solids(&scene);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "Spur gear 20×1");
    let vol = got[0].1.expect("a solid");
    // between the root disc and the tip disc, less the bore, 5 mm thick
    let disc = |d: f64| std::f64::consts::PI * d * d / 4.0 * 5.0;
    assert!(
        vol > disc(17.5) - disc(5.0) && vol < disc(22.0) - disc(5.0),
        "{vol}"
    );
    let (lo, hi) = scene.compute_bounds().expect("geometry");
    // the tip circle is m (N + 2) = 22 mm across
    assert!(((hi - lo).x - 0.022).abs() < 1e-4, "{lo} {hi}");
    assert!(((hi - lo).z - 0.005).abs() < 1e-9, "{lo} {hi}");
}
