// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The materials registry (the document round trip and the .skp import tests live
//! in xtrude-io).

use indexmap::IndexMap;
use xtrude_core::attrs::Paint;
use xtrude_model::materials::{register, Material};

#[test]
fn test_material_roundtrip_and_stamp() {
    let mat = Material::new("Matte grey", Some([0.62, 0.60, 0.58]), Some(0.9));
    assert_eq!(Material::from_json(&mat.to_json()), mat);
    let stamp = mat.face_paint();
    assert_eq!(
        stamp,
        Paint {
            color: Some([0.62, 0.60, 0.58]),
            mat: Some("Matte grey".into()),
            opacity: Some(0.9)
        }
    );
}

#[test]
fn test_register_is_idempotent_for_identical_recipes() {
    let mut reg = IndexMap::new();
    assert_eq!(
        register(&mut reg, Material::new("Wood", Some([0.5, 0.3, 0.1]), None)),
        "Wood"
    );
    assert_eq!(
        register(&mut reg, Material::new("Wood", Some([0.5, 0.3, 0.1]), None)),
        "Wood"
    );
    assert_eq!(reg.len(), 1);
}

#[test]
fn test_register_renames_on_recipe_conflict() {
    let mut reg = IndexMap::new();
    register(&mut reg, Material::new("Wood", Some([0.5, 0.3, 0.1]), None));
    let fin = register(&mut reg, Material::new("Wood", Some([0.9, 0.9, 0.9]), None));
    assert_eq!(fin, "Wood (2)");
    assert_eq!(reg["Wood (2)"].name, "Wood (2)");
    assert_eq!(reg.len(), 2);
}
