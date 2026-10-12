// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! STEP files read by the `xtrude-step` helper (OpenCascade) and applied
//! to a scene: dimensions, volumes, units, the hole's fit, the assembly's
//! groups, placements, names and colours, and a malformed file.
//!
//! These need a helper built with OpenCascade
//! (`cargo build --release -p xtrude-step --features occt`); without one
//! they skip, unless `XTRUDE_STEP_REQUIRED` is set (see `common::step`).
//! The fixtures are in `tests/data/step` (see its README).

use crate::common;

use common::step::{fixture, helper};
use xtrude_core::DVec3;
use xtrude_io::step::{
    apply_payload, parse_step_with, HelperBackend, StepError, StepPayload, Tessellation,
};
use xtrude_model::group::{iter_placements, Group};
use xtrude_model::scene::Scene;
use xtrude_model::solids::solid_volume;

const MM: f64 = 1e-3;
const MM3: f64 = 1e-9;

fn parse(helper: &HelperBackend, name: &str) -> StepPayload {
    parse_step_with(helper, &fixture(name), &Tessellation::default(), None, None)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The scene after importing `name`.
fn import(helper: &HelperBackend, name: &str) -> (Scene, StepPayload) {
    let payload = parse(helper, name);
    let mut scene = Scene::new();
    apply_payload(&mut scene, &payload).unwrap();
    (scene, payload)
}

/// Every placement of the scene, depth first, with its world matrix.
fn placements(scene: &Scene) -> Vec<(&Group, xtrude_core::math::DMat4)> {
    scene
        .doc
        .groups
        .iter()
        .flat_map(|g| iter_placements(g, None))
        .map(|(g, m)| (g, m.unwrap_or_default()))
        .collect()
}

/// The world-space corners of every face of the scene.
fn world_points(scene: &Scene) -> Vec<DVec3> {
    let mut out = Vec::new();
    for (g, m) in placements(scene) {
        let mesh = scene.doc.mesh(g.mesh);
        for f in mesh.face_ids() {
            out.extend(
                mesh.face_positions(f)
                    .iter()
                    .map(|&p| m.transform_point3(p)),
            );
        }
    }
    out
}

/// The size of the scene's bounding box, in millimetres.
fn size_mm(scene: &Scene) -> [f64; 3] {
    let pts = world_points(scene);
    let lo = pts.iter().copied().reduce(DVec3::min).unwrap();
    let hi = pts.iter().copied().reduce(DVec3::max).unwrap();
    ((hi - lo) / MM).to_array()
}

fn assert_size(scene: &Scene, expected: [f64; 3], tol_mm: f64) {
    let got = size_mm(scene);
    for k in 0..3 {
        assert!(
            (got[k] - expected[k]).abs() <= tol_mm,
            "size {got:?}, expected {expected:?}"
        );
    }
}

/// The solid volume of the scene's one group (imported from `name`), in
/// mm³.
fn volume_mm3(scene: &Scene, name: &str) -> f64 {
    assert_eq!(scene.doc.groups.len(), 1, "{name}");
    let volume = solid_volume(&scene.doc.meshes, &scene.doc.groups[0]);
    volume.unwrap_or_else(|| panic!("{name} is not a solid")) / MM3
}

fn assert_close(got: f64, expected: f64, rel: f64) {
    assert!(
        ((got - expected) / expected).abs() <= rel,
        "{got} differs from {expected} by more than {}%",
        rel * 100.0
    );
}

#[test]
fn test_the_cube_has_its_size_and_volume() {
    let Some(h) = helper() else { return };
    let (scene, payload) = import(&h, "cube.step");
    assert!(payload.warnings.is_empty(), "{:?}", payload.warnings);
    assert_eq!(scene.doc.groups[0].name, "Cube");
    assert_size(&scene, [10.0, 10.0, 10.0], 1e-6);
    assert_close(volume_mm3(&scene, "cube.step"), 1000.0, 1e-9);
    let mesh = scene.doc.mesh(scene.doc.groups[0].mesh);
    assert_eq!(mesh.faces.len(), 6, "one face per side");
}

#[test]
fn test_the_hole_keeps_its_diameter_within_the_chord_tolerance() {
    let Some(h) = helper() else { return };
    let (scene, _) = import(&h, "plate_hole.step");
    assert_size(&scene, [40.0, 30.0, 5.0], 1e-6);
    // The top face: one polygon, the hole its one inner ring.
    let mesh = scene.doc.mesh(scene.doc.groups[0].mesh);
    let top = mesh
        .face_ids()
        .into_iter()
        .find(|&f| {
            mesh.face_positions(f)
                .iter()
                .all(|p| (p.z - 5.0 * MM).abs() < 1e-9)
        })
        .expect("the top face");
    let holes = mesh.face_hole_positions(top);
    assert_eq!(holes.len(), 1);
    let ring = &holes[0];
    let centre = DVec3::new(20.0 * MM, 15.0 * MM, 5.0 * MM);
    let chord = Tessellation::default().chord;
    for (i, &p) in ring.iter().enumerate() {
        // Corners on the circle, sides no deeper inside than the chord.
        assert!(
            ((p - centre).length() - 5.0 * MM).abs() < 1e-7,
            "corner {i}"
        );
        let mid = (p + ring[(i + 1) % ring.len()]) / 2.0;
        assert!(
            (mid - centre).length() >= 5.0 * MM - chord - 1e-9,
            "side {i}"
        );
    }
    assert!(ring.len() >= 36, "{} sides", ring.len());
    assert_close(
        volume_mm3(&scene, "plate_hole.step"),
        40.0 * 30.0 * 5.0 - 25.0 * std::f64::consts::PI * 5.0,
        2e-4,
    );
}

#[test]
fn test_an_inch_file_is_scaled_to_metres() {
    let Some(h) = helper() else { return };
    let (scene, _) = import(&h, "plate_inch.step");
    assert_size(&scene, [50.8, 38.1, 6.35], 1e-6);
    let (nist, _) = import(&h, "nist_ftc_09_asme1_rd.stp");
    assert_size(&nist, [190.5, 3.0378, 279.4], 1e-3);
}

#[test]
fn test_curved_parts_import_as_solids_of_the_right_volume() {
    let Some(h) = helper() else { return };
    // The exact volumes OpenCascade reports for these shapes.
    for (name, exact) in [
        ("cylinder_fillet.step", 9373.2521),
        ("bspline_loft.step", 19015.9869),
        ("nist_ftc_11_asme1_rb.stp", 5129.3861),
        ("nist_ftc_09_asme1_rd.stp", 136445.1048),
    ] {
        let (scene, payload) = import(&h, name);
        assert!(
            payload.warnings.is_empty(),
            "{name}: {:?}",
            payload.warnings
        );
        assert_close(volume_mm3(&scene, name), exact, 5e-3);
    }
}

#[test]
fn test_the_assembly_keeps_its_parts_placements_names_and_colours() {
    let Some(h) = helper() else { return };
    let (scene, payload) = import(&h, "assembly_colours.step");
    assert!(payload.warnings.is_empty(), "{:?}", payload.warnings);
    assert_eq!(scene.doc.groups.len(), 1);
    let top = &scene.doc.groups[0];
    assert_eq!(top.name, "Assembly");
    let names: Vec<&str> = top.children.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["BracketLeft", "BracketRight", "Base"]);
    let (left, right, base) = (&top.children[0], &top.children[1], &top.children[2]);

    // The bracket is one part placed twice: two component instances of
    // one mesh (its one body folded into it).
    assert!(left.component && right.component && !base.component);
    assert!(left.children.is_empty() && right.children.is_empty());
    assert_eq!(left.mesh, right.mesh);
    let r = right.xform.unwrap();
    assert!((r.w_axis.truncate() - DVec3::new(50.0 * MM, 0.0, 0.0)).length() < 1e-12);
    assert!(
        (r.x_axis.truncate() - DVec3::Y).length() < 1e-9,
        "turned 90° about Z"
    );

    let body = left;
    let volume = solid_volume(&scene.doc.meshes, body).expect("a solid") / MM3;
    assert_close(
        volume,
        20.0 * 10.0 * 4.0 - 4.0 * std::f64::consts::PI * 4.0,
        2e-3,
    );

    // Red with a green hole; the base blue.
    let colors = |g: &Group| -> Vec<[f64; 3]> {
        let mesh = scene.doc.mesh(g.mesh);
        mesh.face_ids()
            .iter()
            .map(|&f| mesh.face(f).attrs.paint.color.expect("painted"))
            .collect()
    };
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3);
    let body_colors = colors(body);
    let green = body_colors
        .iter()
        .filter(|c| near(**c, [0.1, 0.7, 0.2]))
        .count();
    let red = body_colors
        .iter()
        .filter(|c| near(**c, [0.8, 0.1, 0.1]))
        .count();
    assert!(green > 0 && red > 0 && green + red == body_colors.len());
    assert!(colors(base).iter().all(|c| near(*c, [0.1, 0.2, 0.8])));

    // The whole assembly: 80 × 30 × 6 mm.
    assert_size(&scene, [80.0, 30.0, 6.0], 1e-6);
}

#[test]
fn test_a_malformed_file_fails_without_a_payload() {
    let Some(h) = helper() else { return };
    let dir = common::TmpDir::new();
    let path = dir.join("broken.step");
    std::fs::write(&path, "ISO-10303-21;\nHEADER;\nthis is not STEP\n").unwrap();
    let result = parse_step_with(&h, &path, &Tessellation::default(), None, None);
    assert!(
        matches!(result, Err(StepError::Read(_) | StepError::NoGeometry)),
        "{result:?}"
    );
}
