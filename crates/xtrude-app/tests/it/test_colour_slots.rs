// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Colors panel lists the model's colours as filament slots 1, 2, 3…,
//! in the order the 3MF export writes them, each with its face area.

use crate::common;

use common::{v3, window};
use xtrude_app::tools::paint::paint_state;
use xtrude_app::tray::colors::Colors;
use xtrude_core::attrs::Paint;
use xtrude_core::Mesh;
use xtrude_io::threemf::colour_slots;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::units::{bind, default_model_units, Units};

/// A `w` × `h` m rectangle at height `z`.
fn rect(m: &mut Mesh, w: f64, h: f64, z: f64) -> xtrude_core::FaceId {
    m.add_face(
        &[v3(0., 0., z), v3(w, 0., z), v3(w, h, z), v3(0., h, z)],
        &[],
    )
}

/// Loose geometry: an unpainted 1 cm² square, then a 2 cm² one in
/// «Signal red»; a group of 3 cm² painted blue.
fn model() -> xtrude_app::window::MainWindow {
    let mut w = window();
    let mesh = w.viewport.scene.mesh_mut();
    rect(mesh, 0.01, 0.01, 0.);
    let red = rect(mesh, 0.02, 0.01, 0.05);
    mesh.face_mut(red).attrs.paint = Paint {
        color: Some([0.8, 0.1, 0.1]),
        mat: Some("Signal red".into()),
        opacity: None,
    };
    let mut m = Mesh::new();
    rect(&mut m, 0.03, 0.01, 0.1);
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some("Lid"));
    g.material = Some(Paint {
        color: Some([0.1, 0.2, 0.9]),
        mat: None,
        opacity: None,
    });
    w.viewport.scene.doc.groups.push(g);
    w
}

#[test]
fn test_the_slots_are_numbered_in_the_export_order_with_their_areas() {
    let w = model();
    let mut p = Colors::default();
    p.refresh_in_model(&w);
    let numbers: Vec<usize> = p.slots.iter().map(|s| s.number).collect();
    assert_eq!(numbers, [1, 2, 3]);
    let export: Vec<[f64; 3]> = colour_slots(&w.viewport.scene)
        .iter()
        .map(|c| c.key.color)
        .collect();
    let panel: Vec<[f64; 3]> = p.slots.iter().map(|s| s.swatch.rgb).collect();
    assert_eq!(panel, export);
    assert!(p.slots[0].unpainted);
    assert_eq!(p.slots[1].swatch.tip, "Signal red");
    assert_eq!(p.slots[2].swatch.tip, "Color");
    for (slot, cm2) in p.slots.iter().zip([1.0, 2.0, 3.0]) {
        assert!((slot.area - cm2 * 1e-4).abs() < 1e-12, "{}", slot.area);
    }
    assert_eq!(p.slots_title(), "Filament slots  (3)");
}

#[test]
fn test_a_slot_area_is_in_the_model_units() {
    let w = model();
    let mut p = Colors::default();
    p.refresh_in_model(&w);
    bind(&Units::new("cm", 1));
    let text = p.slots[1].area_text();
    bind(&default_model_units());
    assert_eq!(text, "2.0 cm²");
}

#[test]
fn test_picking_the_unpainted_slot_paints_the_colour_off() {
    let mut w = model();
    let mut p = Colors::default();
    p.refresh_in_model(&w);
    let red = p.slots[1].clone();
    p.apply_slot(&mut w, &red);
    let st = paint_state();
    assert!(!st.is_default);
    assert_eq!(
        st.named_color.as_ref().map(|m| m.name.as_str()),
        Some("Signal red")
    );
    let unpainted = p.slots[0].clone();
    p.apply_slot(&mut w, &unpainted);
    assert!(paint_state().is_default);
}
